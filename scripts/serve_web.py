#!/usr/bin/env python3
"""
Serves web-based telemetry visualization tools from the web/ directory.

This simple HTTP server provides a convenient way to serve HTML-based
visualization tools that connect to the MQTT broker via WebSockets (port 9001).
The server automatically lists all available tools in a directory browser.

USAGE:
    python3 serve_web.py [port]
    Default port: 8888

    Then open: http://localhost:8888

ADDING NEW TOOLS:
    1. Create your HTML visualization in scripts/web/
    2. In your JavaScript, connect to MQTT WebSocket:
       const ws = 'ws://telemetry.local:9001';
       const client = mqtt.connect(ws);
    3. Subscribe to topics: can/# or metrics/# or driver/#
    4. Run this server and access your tool via the directory listing

EXAMPLE TOOLS:
    - track.html: Live track map showing car position and telemetry

NOTE:
    Tools connect to telemetry.local via mDNS, so serve_web.py can run on
    any machine on the LAN - it just serves static files.
"""

import http.server
import os
import socketserver
import sys

PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8888

# Change to web directory to serve HTML tools
script_dir = os.path.dirname(os.path.abspath(__file__))
web_dir = os.path.join(script_dir, "web")
os.chdir(web_dir)

Handler = http.server.SimpleHTTPRequestHandler

with socketserver.TCPServer(("", PORT), Handler) as httpd:
    print(f"Serving web visualization tools at http://localhost:{PORT}")
    print("Available tools will be listed in the directory")
    print("Press Ctrl+C to stop")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nStopped.")
