#!/usr/bin/env python3
"""USB bridge for running RaceStudio 2 under Wine with the MyChron 4 data key.

Wine can't pass RS2's requests through AiM's kernel driver (the driver gets a
pointer into RS2's memory that it can't follow from winedevice.exe), so a
small DLL in RS2 (aimhook.dll, see hook.c) sends each request here over a
local TCP connection, and this does the USB transfer with libusb.

Every request and reply is logged to aimkey-<time>.log as hex, which is the
point: it's the protocol RS2 speaks to the key.

Protocol (little-endian), one request then one reply per message:
  request:  u32 kind, u32 endpoint, u32 timeout_ms, u8 setup[8], u32 len, data[len for OUT]
            kind 1 = control, 2 = bulk/interrupt OUT, 3 = bulk/interrupt IN
  reply:    i32 status (bytes transferred or negative libusb error), data[status for IN]

Local use only: listens on 127.0.0.1.
"""
import ctypes, ctypes.util, socket, struct, sys, time, os, threading

VID, PID = 0x11CC, 0x0101
PORT = 47321

lib = ctypes.CDLL(ctypes.util.find_library("usb-1.0") or "libusb-1.0.so.0")
lib.libusb_open_device_with_vid_pid.restype = ctypes.c_void_p
lib.libusb_open_device_with_vid_pid.argtypes = [ctypes.c_void_p, ctypes.c_uint16, ctypes.c_uint16]
lib.libusb_control_transfer.argtypes = [ctypes.c_void_p, ctypes.c_uint8, ctypes.c_uint8, ctypes.c_uint16, ctypes.c_uint16, ctypes.c_char_p, ctypes.c_uint16, ctypes.c_uint]
lib.libusb_bulk_transfer.argtypes = [ctypes.c_void_p, ctypes.c_ubyte, ctypes.c_char_p, ctypes.c_int, ctypes.POINTER(ctypes.c_int), ctypes.c_uint]
lib.libusb_interrupt_transfer.argtypes = lib.libusb_bulk_transfer.argtypes
lib.libusb_claim_interface.argtypes = [ctypes.c_void_p, ctypes.c_int]
lib.libusb_error_name.restype = ctypes.c_char_p

log = open(f"aimkey-{time.strftime('%Y%m%d-%H%M%S')}.log", "w", buffering=1)


def say(line):
    print(line)
    log.write(line + "\n")


def open_key():
    assert lib.libusb_init(None) == 0
    h = lib.libusb_open_device_with_vid_pid(None, VID, PID)
    if not h:
        sys.exit("data key (11cc:0101) not found or no permission")
    r = lib.libusb_claim_interface(h, 0)
    say(f"opened key; claim interface 0 -> {r} {lib.libusb_error_name(r).decode() if r else ''}")
    return h


def recv_exact(c, n):
    b = b""
    while len(b) < n:
        p = c.recv(n - len(b))
        if not p:
            raise EOFError
        b += p
    return b


USB_LOCK = threading.Lock()


def client(h, c, name):
    say(f"--- {name} connected")
    try:
        while True:
            kind, ep, timeout, setup, n = struct.unpack("<III8sI", recv_exact(c, 24))
            out = recv_exact(c, n) if kind in (1, 2) and (kind == 2 or not setup[0] & 0x80) else b""
            buf = ctypes.create_string_buffer(out, max(n, 1)) if out else ctypes.create_string_buffer(max(n, 1))
            tmo = timeout or 5000
            with USB_LOCK:
                if kind == 1:
                    rt, rq, val, idx, ln = struct.unpack("<BBHHH", setup)
                    # AiM's driver sends RS2's vendor requests to the endpoint
                    # recipient (0x40 -> 0x42), as seen on the wire under Windows' driver.
                    if rt == 0x40:
                        rt = 0x42
                        setup = bytes([rt]) + setup[1:]
                    r = lib.libusb_control_transfer(h, rt, rq, val, idx, buf, ln, tmo)
                    data = buf.raw[: max(r, 0)] if rt & 0x80 else b""
                    say(f"{name} CTRL {setup.hex()} out={out.hex()} -> {r} {data.hex()}")
                else:
                    got = ctypes.c_int(0)
                    xfer = lib.libusb_bulk_transfer if ep & 0x7F in (2, 3) else lib.libusb_interrupt_transfer
                    r = xfer(h, ep, buf, n, ctypes.byref(got), tmo)
                    r = got.value if r == 0 or got.value else r
                    data = buf.raw[: max(r, 0)] if ep & 0x80 else b""
                    say(f"{name} {'IN ' if ep & 0x80 else 'OUT'} ep{ep:02x} n={n} out={out.hex()} -> {r} {data.hex()}")
            c.sendall(struct.pack("<i", r) + data)
    except (EOFError, ConnectionResetError, BrokenPipeError):
        say(f"--- {name} disconnected")
        c.close()


def serve(h):
    s = socket.socket()
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", PORT))
    s.listen(4)
    say(f"listening on 127.0.0.1:{PORT}")
    n = 0
    while True:
        c, _ = s.accept()
        n += 1
        threading.Thread(target=client, args=(h, c, f"#{n}"), daemon=True).start()


if __name__ == "__main__":
    serve(open_key())
