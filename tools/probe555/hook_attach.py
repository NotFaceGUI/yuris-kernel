# Attach to an already-running v555 official engine and install hook_v555.js.
# Usage: python -I hook_attach.py <pid> <out.log> [seconds]
import ctypes
import json
import os
import sys
import time
from ctypes import wintypes

import frida

pid = int(sys.argv[1])
out = os.path.abspath(sys.argv[2])
seconds = int(sys.argv[3]) if len(sys.argv) > 3 else 20

log = open(out, "w", encoding="utf-8")


def on_message(message, data):
    if message.get("type") == "send":
        log.write(json.dumps(message["payload"], ensure_ascii=False) + "\n")
    else:
        log.write(json.dumps(message, ensure_ascii=False) + "\n")
    log.flush()


session = frida.attach(pid)
script = session.create_script(
    open(os.path.join(os.path.dirname(__file__), "hook_v555.js"), encoding="utf-8").read()
)
script.on("message", on_message)
script.load()

user32 = ctypes.WinDLL("user32", use_last_error=True)


def find_window(pid):
    found = []

    @ctypes.WINFUNCTYPE(wintypes.BOOL, wintypes.HWND, wintypes.LPARAM)
    def cb(hwnd, _):
        p = wintypes.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(p))
        if p.value == pid and user32.IsWindowVisible(hwnd):
            found.append(hwnd)
        return True

    user32.EnumWindows(cb, 0)
    return found[0] if found else None


def poke(pid):
    h = find_window(pid)
    if not h:
        return
    user32.SetForegroundWindow(h)
    user32.PostMessageW(h, 0x0100, 0x0D, 0x00000001)
    user32.PostMessageW(h, 0x0101, 0x0D, 0xC0000001)
    lp = (360 << 16) | 640
    user32.PostMessageW(h, 0x0200, 0, lp)
    user32.PostMessageW(h, 0x0201, 1, lp)
    user32.PostMessageW(h, 0x0202, 0, lp)


start = time.time()
while time.time() - start < seconds:
    time.sleep(3)
    poke(pid)
try:
    session.detach()
except Exception as e:
    print("detach", e)
log.close()
print("done")
