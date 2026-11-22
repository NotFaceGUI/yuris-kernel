# Launch a v555 official engine suspended, attach hooks before any boot code,
# then resume and drive it to the title/START.
# Usage: python -I hook_boot.py <exe> <out.log> [seconds]
import ctypes
import json
import os
import subprocess
import sys
import time
from ctypes import wintypes

import frida

exe = os.path.abspath(sys.argv[1])
out = os.path.abspath(sys.argv[2])
seconds = int(sys.argv[3]) if len(sys.argv) > 3 else 40

log = open(out, "w", encoding="utf-8")


def on_message(message, data):
    if message.get("type") == "send":
        log.write(json.dumps(message["payload"], ensure_ascii=False) + "\n")
    else:
        log.write(json.dumps(message, ensure_ascii=False) + "\n")
    log.flush()


CREATE_SUSPENDED = 0x00000004
p = subprocess.Popen([exe], cwd=os.path.dirname(exe), creationflags=CREATE_SUSPENDED)
print("pid", p.pid, "suspended")

session = frida.attach(p.pid)
script = session.create_script(
    open(os.path.join(os.path.dirname(__file__), "hook_v555.js"), encoding="utf-8").read()
)
script.on("message", on_message)
script.load()
print("hooked")

ntdll = ctypes.WinDLL("ntdll")
status = ntdll.NtResumeProcess(p._handle)
print("resume status", status)

user32 = ctypes.WinDLL("user32", use_last_error=True)


def find_window(pid):
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, _):
        q = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(q))
        if q.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    user32.EnumWindows(cb, 0)
    return found[0] if found else None


class POINT(ctypes.Structure):
    _fields_ = [("x", wintypes.LONG), ("y", wintypes.LONG)]


def click(h, cx, cy):
    pt = POINT(cx, cy)
    user32.ClientToScreen(h, ctypes.byref(pt))
    user32.SetCursorPos(pt.x, pt.y)
    time.sleep(0.05)
    user32.mouse_event(0x0002, 0, 0, 0, 0)
    time.sleep(0.05)
    user32.mouse_event(0x0004, 0, 0, 0, 0)


def enter():
    user32.keybd_event(0x0D, 0x1C, 0, 0)
    time.sleep(0.05)
    user32.keybd_event(0x0D, 0x1C, 0x0002, 0)


start = time.time()
stage = 0
while time.time() - start < seconds:
    time.sleep(1)
    h = find_window(p.pid)
    if not h:
        continue
    elapsed = time.time() - start
    # Logo animation runs ~14 s on this machine; then title START is at client (60, 455).
    if elapsed > 12 and stage == 0:
        user32.ShowWindow(h, 9)
        user32.SetForegroundWindow(h)
        stage = 1
    if elapsed > 14 and stage == 1:
        click(h, 60, 455)
        enter()
        stage = 2
    elif elapsed > 18 and stage == 2:
        click(h, 60, 455)
        enter()
        stage = 3
    elif elapsed > 24 and stage == 3:
        # advance dialogue a few times
        enter()
        time.sleep(0.4)
        enter()

try:
    session.detach()
except Exception as e:
    print("detach", e)
log.close()
print("done")
