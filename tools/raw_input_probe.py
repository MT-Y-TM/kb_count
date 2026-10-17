"""Short Windows-only Raw Input control experiment; no hooks or data files.

Run with a standard-user Python x64 interpreter while kb_count stays running:
    python tools/raw_input_probe.py --target-pid GAME_PID --seconds 120

This separate process registers its own hidden INPUTSINK window. Registrations
are per process; it does not change kb_count's registration or write its DB.
Only aggregate packet/button totals are printed. Keyboard codes/text, mouse
movement values, coordinates, titles and input sequences are never retained.
Keyboard make totals include repeats and are NOT physical-press statistics.
"""

import argparse
import ctypes as c
from ctypes import wintypes as w
import json
import os
import struct
import time


def run(target_pid: int, seconds: int) -> None:
    if os.name != "nt" or c.sizeof(c.c_void_p) != 8:
        raise RuntimeError("This probe requires Windows x64 Python")
    user = c.WinDLL("user32", use_last_error=True)
    kernel = c.WinDLL("kernel32", use_last_error=True)
    callback_type = c.WINFUNCTYPE(c.c_ssize_t, w.HWND, w.UINT, w.WPARAM, w.LPARAM)

    class WindowClass(c.Structure):
        _fields_ = [("style", w.UINT), ("proc", callback_type),
                    ("class_extra", c.c_int), ("window_extra", c.c_int),
                    ("instance", w.HINSTANCE), ("icon", w.HICON),
                    ("cursor", w.HANDLE), ("background", w.HBRUSH),
                    ("menu", w.LPCWSTR), ("name", w.LPCWSTR)]

    class Device(c.Structure):
        _fields_ = [("page", w.USHORT), ("usage", w.USHORT),
                    ("flags", w.DWORD), ("target", w.HWND)]

    class Header(c.Structure):
        _fields_ = [("kind", w.DWORD), ("size", w.DWORD),
                    ("device", w.HANDLE), ("parameter", w.WPARAM)]

    def api(dll, name, args, result):
        fn = getattr(dll, name)
        fn.argtypes, fn.restype = args, result
        return fn

    foreground = api(user, "GetForegroundWindow", [], w.HWND)
    window_pid = api(user, "GetWindowThreadProcessId", [w.HWND, c.POINTER(w.DWORD)], w.DWORD)
    default_proc = api(user, "DefWindowProcW", [w.HWND, w.UINT, w.WPARAM, w.LPARAM], c.c_ssize_t)
    get_raw = api(user, "GetRawInputData", [w.HANDLE, w.UINT, c.c_void_p, c.POINTER(w.UINT), w.UINT], w.UINT)
    quit_loop = api(user, "PostQuitMessage", [c.c_int], None)
    register_class = api(user, "RegisterClassW", [c.POINTER(WindowClass)], w.ATOM)
    unregister_class = api(user, "UnregisterClassW", [w.LPCWSTR, w.HINSTANCE], w.BOOL)
    create_window = api(user, "CreateWindowExW", [w.DWORD, w.LPCWSTR, w.LPCWSTR, w.DWORD,
                        c.c_int, c.c_int, c.c_int, c.c_int, w.HWND, w.HMENU, w.HINSTANCE, c.c_void_p], w.HWND)
    destroy_window = api(user, "DestroyWindow", [w.HWND], w.BOOL)
    register_devices = api(user, "RegisterRawInputDevices", [c.POINTER(Device), w.UINT, w.UINT], w.BOOL)
    set_timer = api(user, "SetTimer", [w.HWND, c.c_size_t, w.UINT, c.c_void_p], c.c_size_t)
    kill_timer = api(user, "KillTimer", [w.HWND, c.c_size_t], w.BOOL)
    get_message = api(user, "GetMessageW", [c.POINTER(w.MSG), w.HWND, w.UINT, w.UINT], c.c_int)
    dispatch = api(user, "DispatchMessageW", [c.POINTER(w.MSG)], c.c_ssize_t)
    module = api(kernel, "GetModuleHandleW", [w.LPCWSTR], w.HMODULE)(None)
    advapi = c.WinDLL("advapi32", use_last_error=True)
    open_token = api(advapi, "OpenProcessToken", [w.HANDLE, w.DWORD, c.POINTER(w.HANDLE)], w.BOOL)
    token_info = api(advapi, "GetTokenInformation", [w.HANDLE, c.c_int, c.c_void_p, w.DWORD,
                                                  c.POINTER(w.DWORD)], w.BOOL)
    close_handle = api(kernel, "CloseHandle", [w.HANDLE], w.BOOL)

    def elevation_for_process(process):
        token, elevated, returned = w.HANDLE(), w.DWORD(), w.DWORD()
        if not open_token(process, 0x0008, c.byref(token)):
            return "unavailable"
        try:
            if not token_info(token, 20, c.byref(elevated), c.sizeof(elevated), c.byref(returned)):
                return "unavailable"
            if returned.value < c.sizeof(elevated):
                return "unavailable"
            return "elevated" if elevated.value else "standard"
        finally:
            close_handle(token)

    if elevation_for_process(w.HANDLE(-1)) != "standard":
        raise RuntimeError("Use verified standard-user Python to match normal kb_count permissions")
    open_process = api(kernel, "OpenProcess", [w.DWORD, w.BOOL, w.DWORD], w.HANDLE)
    exit_code = api(kernel, "GetExitCodeProcess", [w.HANDLE, c.POINTER(w.DWORD)], w.BOOL)
    target_process = open_process(0x1000, False, target_pid)
    if not target_process:
        raise c.WinError(c.get_last_error())
    status = w.DWORD()

    def target_alive():
        return bool(exit_code(target_process, c.byref(status))) and status.value == 259

    if not target_alive():
        close_handle(target_process)
        raise RuntimeError("The target process has already exited")
    target_elevation = elevation_for_process(target_process)
    buckets = {name: dict(keyboard_packets=0, keyboard_make_packets=0,
                         mouse_packets=0, mouse_button_downs=0, wheel_packets=0)
               for name in ("target_foreground", "other_foreground")}
    errors = 0
    target_samples = 0
    callback_errors = []
    started = time.monotonic()

    def is_target():
        pid = w.DWORD()
        window_pid(foreground(), c.byref(pid))
        return pid.value == target_pid and target_alive()

    def summary(final=False):
        print(json.dumps(dict(elapsed_seconds=round(time.monotonic() - started),
                              process_elevation="standard", target_alive=target_alive(),
                              target_elevation=target_elevation,
                              target_foreground_now=is_target(),
                              target_foreground_samples=target_samples,
                              raw_data_errors=errors, final=final, totals=buckets)), flush=True)

    @callback_type
    def procedure(hwnd, msg, wp, lp):
        nonlocal errors, target_samples
        try:
            if msg == 0x00FF:  # WM_INPUT; read before DefWindowProc cleanup.
                buffer = (c.c_uint64 * 16)()  # DWORD-aligned, bounded, zeroed.
                capacity = w.UINT(c.sizeof(buffer))
                read = get_raw(w.HANDLE(lp), 0x10000003, buffer, c.byref(capacity), c.sizeof(Header))
                if read == 0xFFFFFFFF or not c.sizeof(Header) <= read <= c.sizeof(buffer):
                    errors += 1
                else:
                    header = Header.from_buffer(buffer)
                    offset = c.sizeof(Header)
                    if header.size != read:
                        errors += 1
                    elif header.kind in (0, 1):
                        minimum = 24 if header.kind == 0 else 16
                        if read < offset + minimum:
                            errors += 1
                        else:
                            bucket = buckets["target_foreground" if is_target() else "other_foreground"]
                            if header.kind == 1:
                                flags = struct.unpack_from("<H", buffer, offset + 2)[0]
                                bucket["keyboard_packets"] += 1
                                bucket["keyboard_make_packets"] += int(flags & 1 == 0)
                            else:
                                flags = struct.unpack_from("<H", buffer, offset + 4)[0]
                                bucket["mouse_packets"] += 1
                                bucket["mouse_button_downs"] += (flags & 0x0155).bit_count()
                                bucket["wheel_packets"] += int(bool(flags & 0x0C00))
            elif msg == 0x0113:  # WM_TIMER, one aggregate summary per 10 seconds.
                target_samples += int(is_target())
                summary()
                if time.monotonic() - started >= seconds or not target_alive():
                    quit_loop(0)
        except Exception as error:
            callback_errors.append(type(error).__name__)
            quit_loop(1)
        return default_proc(hwnd, msg, wp, lp)

    name = f"kb_count_diagnostic_probe_{os.getpid()}"
    cls = WindowClass(proc=procedure, instance=module, name=name)
    if not register_class(c.byref(cls)):
        error = c.WinError(c.get_last_error())
        close_handle(target_process)
        raise error
    hwnd = None
    registered = False
    try:
        hwnd = create_window(0x80, name, name, 0x80000000, 0, 0, 0, 0, None, None, module, None)
        if not hwnd:
            raise c.WinError(c.get_last_error())
        devices = (Device * 2)(Device(1, 2, 0x2100, hwnd), Device(1, 6, 0x2100, hwnd))
        if not register_devices(devices, 2, c.sizeof(Device)):
            raise c.WinError(c.get_last_error())
        registered = True
        if not set_timer(hwnd, 1, 10_000, None):
            raise c.WinError(c.get_last_error())
        message = w.MSG()
        while True:
            result = get_message(c.byref(message), None, 0, 0)
            if result == -1:
                raise c.WinError(c.get_last_error())
            if result == 0:
                break
            dispatch(c.byref(message))
        if callback_errors:
            raise RuntimeError("Probe callback failed: " + callback_errors[0])
        summary(final=True)
    finally:
        if registered:
            removed = (Device * 2)(Device(1, 2, 1, None), Device(1, 6, 1, None))
            register_devices(removed, 2, c.sizeof(Device))
        if hwnd:
            kill_timer(hwnd, 1)
            destroy_window(hwnd)
        unregister_class(name, module)
        close_handle(target_process)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target-pid", type=int, required=True)
    parser.add_argument("--seconds", type=int, choices=range(10, 601), default=120, metavar="10..600")
    args = parser.parse_args()
    if args.target_pid <= 0:
        parser.error("--target-pid must be positive; recheck the live game process first")
    run(args.target_pid, args.seconds)
