#!/usr/bin/env python3
"""
Test script for sending CAN write commands to can_receiver

Usage:
    ./test_commands.py <serial_port> [count] [delay_ms]
    ./test_commands.py /dev/ttyUSB0 10 100  # Send 10 commands with 100ms delay
    ./test_commands.py /dev/ttyUSB0 1000 0  # Send 1000 commands as fast as possible (stress test)

Requires: pip install pyserial
"""

import json
import sys
import threading
import time

try:
    import serial
except ImportError:
    print("Error: pyserial not installed. Run: pip install pyserial", file=sys.stderr)
    sys.exit(1)


def read_logs(ser, stop_event):
    """Thread function to continuously read and print logs from serial"""
    while not stop_event.is_set():
        try:
            if ser.in_waiting:
                line = ser.readline().decode("utf-8", errors="replace").strip()
                if line:
                    print(f"[LOG] {line}")
        except Exception as e:
            print(f"[ERROR] Read error: {e}", file=sys.stderr)
            break


def main():
    if len(sys.argv) < 2:
        print(
            "Usage: ./test_commands.py <serial_port> [count] [delay_ms]",
            file=sys.stderr,
        )
        print("Example: ./test_commands.py /dev/ttyUSB0 10 100", file=sys.stderr)
        sys.exit(1)

    port = sys.argv[1]
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 10
    delay_ms = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    delay_sec = delay_ms / 1000.0

    print(f"Connecting to {port}...", file=sys.stderr)
    try:
        ser = serial.Serial(port, 115200, timeout=0.1)
    except serial.SerialException as e:
        print(f"Error: Could not open {port}: {e}", file=sys.stderr)
        sys.exit(1)

    # Start log reading thread
    stop_event = threading.Event()
    log_thread = threading.Thread(target=read_logs, args=(ser, stop_event), daemon=True)
    log_thread.start()

    print(
        f"Sending {count} commands with {delay_ms}ms delay between each...",
        file=sys.stderr,
    )
    print("---", file=sys.stderr)

    try:
        for i in range(1, count + 1):
            # Generate CAN ID based on counter (0x100 + i)
            can_id = 0x100 + i

            # Payload: counter as 4 bytes little-endian + index as 4 bytes little-endian
            payload = [format((i >> (8 * j)) & 0xFF, "02x") for j in range(4)] + [
                format((i >> (8 * j)) & 0xFF, "02x") for j in range(4)
            ]

            command = {
                "type": "write",
                "data": {"id": {"standard": f"0x{can_id:03x}"}, "payload": payload},
            }

            json_str = json.dumps(command)
            print(f"[TX {i}/{count}] {json_str}", file=sys.stderr)

            ser.write((json_str + "\n").encode())
            ser.flush()

            if delay_sec > 0:
                time.sleep(delay_sec)

        print("Done sending commands", file=sys.stderr)

    except KeyboardInterrupt:
        print("\nInterrupted by user", file=sys.stderr)

    finally:
        stop_event.set()
        log_thread.join(timeout=1)
        ser.close()


if __name__ == "__main__":
    main()
