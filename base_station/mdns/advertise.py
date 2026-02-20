import logging
import socket
import time

from zeroconf import ServiceInfo, Zeroconf

logging.basicConfig(
    level=logging.DEBUG,
    format="%(asctime)s %(levelname)s %(message)s",
)
log = logging.getLogger(__name__)


def get_local_ip():
    try:
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        s.connect(("192.168.0.1", 80))
        ip = s.getsockname()[0]
        s.close()
        log.info(f"Detected local IP: {ip}")
        return socket.inet_aton(ip)
    except Exception as e:
        log.error(f"Failed to detect local IP: {e}")
        raise


log.info("Starting mDNS advertiser")

ip = get_local_ip()

log.info("Creating ServiceInfo")
info = ServiceInfo(
    "_mqtt._tcp.local.",
    "telemetry._mqtt._tcp.local.",
    addresses=[ip],
    port=1883,
    server="telemetry.local.",  # this registers the hostname
)

log.info("Starting Zeroconf")
zc = Zeroconf()

log.info("Registering service")
zc.register_service(info)
log.info("Service registered: telemetry.local:1883")

try:
    while True:
        time.sleep(1)
except KeyboardInterrupt:
    pass
finally:
    log.info("Unregistering service")
    zc.unregister_service(info)
    zc.close()
    log.info("Done")
