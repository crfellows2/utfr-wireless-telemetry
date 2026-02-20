FROM python:3.12-slim
RUN pip install paho-mqtt
COPY bridge.py .
CMD ["python", "bridge.py"]
