#!/usr/bin/env python3
"""
Auto-discover the OmniScript Raspberry Pi Pico 2 W on the LAN (via its UDP :1425
beacon and/or active subnet UDP probe) and run a full RFC 6455 WebSocket session
against ws://<pico_ip>:1424/.
"""
import base64
import hashlib
import os
import re
import socket
import struct
import sys
import time


WS_GUID = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11"


def discover_pico2w(timeout_sec: float = 25.0) -> tuple[str, int]:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_BROADCAST, 1)
    sock.bind(("0.0.0.0", 1425))
    sock.settimeout(0.5)

    deadline = time.time() + timeout_sec
    next_probe = 0.0
    print(f"[*] Listening on UDP :1425 for OmniScript Pico 2 W beacon (up to {timeout_sec:.0f}s)...")

    while time.time() < deadline:
        now = time.time()
        if now >= next_probe:
            # Send active broadcast probe on 192.168.1.255:1425 and 255.255.255.255:1425
            for bcast in ("192.168.1.255", "255.255.255.255"):
                try:
                    sock.sendto(b"OMNI_DISCOVER?", (bcast, 1425))
                except OSError:
                    pass
            next_probe = now + 1.5

        try:
            data, (src_ip, _src_port) = sock.recvfrom(512)
        except socket.timeout:
            continue

        text = data.decode("utf-8", errors="replace").strip()
        if text == "OMNI_DISCOVER?":
            continue
        m = re.search(r"OMNI_PICO2W\s+ip=([0-9.]+)\s+ws_port=(\d+)", text)
        if m:
            ip, port = m.group(1), int(m.group(2))
            sock.close()
            print(f"[+] Discovered Pico 2 W at {ip}:{port} (from {src_ip}): {text}")
            return ip, port

    sock.close()
    raise TimeoutError("Timed out waiting for Pico 2 W UDP discovery beacon on :1425")


class MinimalWebSocketClient:
    """Dependency-free RFC 6455 client for deterministic verification."""

    def __init__(self, host: str, port: int, timeout: float = 5.0):
        self.host = host
        self.port = port
        self.sock = socket.create_connection((host, port), timeout=timeout)
        self.sock.settimeout(timeout)
        self._handshake()

    def _handshake(self) -> None:
        raw_key = os.urandom(16)
        ws_key = base64.b64encode(raw_key).decode("ascii")
        expected_accept = base64.b64encode(
            hashlib.sha1((ws_key + WS_GUID).encode("ascii")).digest()
        ).decode("ascii")

        req = (
            f"GET / HTTP/1.1\r\n"
            f"Host: {self.host}:{self.port}\r\n"
            f"Upgrade: websocket\r\n"
            f"Connection: Upgrade\r\n"
            f"Sec-WebSocket-Key: {ws_key}\r\n"
            f"Sec-WebSocket-Version: 13\r\n\r\n"
        )
        self.sock.sendall(req.encode("ascii"))

        buf = b""
        while b"\r\n\r\n" not in buf:
            chunk = self.sock.recv(1024)
            if not chunk:
                raise ConnectionError("EOF before WebSocket HTTP 101 response")
            buf += chunk

        hdr_text = buf.split(b"\r\n\r\n", 1)[0].decode("utf-8", errors="replace")
        if "101 Switching Protocols" not in hdr_text:
            raise RuntimeError(f"Unexpected HTTP upgrade response:\n{hdr_text}")
        if expected_accept not in hdr_text:
            raise RuntimeError(
                f"Sec-WebSocket-Accept mismatch!\nExpected: {expected_accept}\nGot:\n{hdr_text}"
            )
        print(f"[+] RFC 6455 Handshake OK! (Sec-WebSocket-Accept: {expected_accept})")

    def send_text(self, text: str) -> None:
        payload = text.encode("utf-8")
        mask = os.urandom(4)
        header = bytearray([0x81])
        n = len(payload)
        if n <= 125:
            header.append(0x80 | n)
        else:
            header.append(0x80 | 126)
            header.extend(struct.pack("!H", n))
        header.extend(mask)
        masked = bytes(b ^ mask[i & 3] for i, b in enumerate(payload))
        self.sock.sendall(bytes(header) + masked)

    def recv_text(self) -> str:
        hdr = self._recv_exact(2)
        opcode = hdr[0] & 0x0F
        masked = (hdr[1] & 0x80) != 0
        plen = hdr[1] & 0x7F
        if plen == 126:
            plen = struct.unpack("!H", self._recv_exact(2))[0]
        mask_key = self._recv_exact(4) if masked else b""
        payload = bytearray(self._recv_exact(plen))
        if masked:
            for i in range(plen):
                payload[i] ^= mask_key[i & 3]
        if opcode == 0x8:
            raise ConnectionError("Server sent WebSocket Close")
        return payload.decode("utf-8", errors="replace")

    def _recv_exact(self, n: int) -> bytes:
        out = bytearray()
        while len(out) < n:
            chunk = self.sock.recv(n - len(out))
            if not chunk:
                raise ConnectionError("Unexpected EOF reading WebSocket frame")
            out.extend(chunk)
        return bytes(out)

    def close(self) -> None:
        try:
            mask = os.urandom(4)
            self.sock.sendall(bytes([0x88, 0x80]) + mask)
        except OSError:
            pass
        self.sock.close()


def main() -> None:
    if len(sys.argv) >= 2:
        ip = sys.argv[1]
        port = int(sys.argv[2]) if len(sys.argv) >= 3 else 1424
    else:
        ip, port = discover_pico2w(timeout_sec=30.0)

    ws = MinimalWebSocketClient(ip, port)
    for cmd in ["status", "led:off", "led:on", "led:toggle", "Hello from Arch Linux over OmniNet!"]:
        print(f" -> TX: {cmd}")
        ws.send_text(cmd)
        reply = ws.recv_text()
        print(f" <- RX: {reply}")
        time.sleep(0.25)
    ws.close()
    print("[+] WebSocket session completed cleanly.")


if __name__ == "__main__":
    main()
