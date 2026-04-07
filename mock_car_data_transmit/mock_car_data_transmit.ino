#include "SparkFun_BMI270_Arduino_Library.h"
#include <Wire.h>
#include <driver/twai.h>

BMI270 imu;
uint8_t i2cAddress = BMI2_I2C_SEC_ADDR;

// Initialing CAN and IMU Parameters
#define CAN_TX_PIN GPIO_NUM_18
#define CAN_RX_PIN GPIO_NUM_19

#define CAN_ID_ACCEL 0x100
#define CAN_ID_GYRO 0x101

#define ACCEL_SCALE 2048.0f
#define GYRO_SCALE 16.384f

// Converting Data Buffer to CAN Payload
void packFloat(uint8_t *buf, int offset, float value, float scale) {
  int16_t raw = (int16_t)constrain(value * scale, -32767.0f, 32767.0f);
  buf[offset] = (raw >> 8) & 0xFF;
  buf[offset + 1] = raw & 0xFF;
}

// Transmit CAN Message
void sendCAN(uint32_t id, uint8_t *data, uint8_t len) {
  twai_message_t msg;
  msg.identifier = id;
  msg.extd = 0;
  msg.rtr = 0;
  msg.ss = 1;
  msg.self = 0;
  msg.dlc_non_comp = 0;
  msg.data_length_code = len;
  memcpy(msg.data, data, len);

  if (twai_transmit(&msg, pdMS_TO_TICKS(1000)) != ESP_OK) {
    Serial.println("CAN TX error");
  }
}

// Parsing CAN Message
void parse_rx(twai_message_t &message) {
  Serial.printf("ID: %" PRIx32 "\nByte:", message.identifier);
  if (!(message.rtr)) {
    for (int i = 0; i < message.data_length_code; i++) {
      Serial.printf(" %d = %02x,", i, message.data[i]);
    }
    Serial.println("");
  }
}

void setup() {
  Serial.begin(115200);

  // Init CAN Connection
  twai_general_config_t g_config =
      TWAI_GENERAL_CONFIG_DEFAULT(CAN_TX_PIN, CAN_RX_PIN, TWAI_MODE_NORMAL);
  twai_timing_config_t t_config = TWAI_TIMING_CONFIG_1MBITS(); // 1 Mbps
  twai_filter_config_t f_config = TWAI_FILTER_CONFIG_ACCEPT_ALL();

  if (twai_driver_install(&g_config, &t_config, &f_config) != ESP_OK) {
    Serial.println("Error: TWAI driver install failed!");
    while (1)
      ;
  }
  if (twai_start() != ESP_OK) {
    Serial.println("Error: TWAI start failed!");
    while (1)
      ;
  }
  Serial.println("CAN (TWAI) initialized!");

  // Reconfigure alerts to detect TX alerts and Bus-Off errors
  uint32_t alerts_to_enable = TWAI_ALERT_TX_IDLE | TWAI_ALERT_TX_SUCCESS |
                              TWAI_ALERT_TX_FAILED | TWAI_ALERT_ERR_PASS |
                              TWAI_ALERT_BUS_ERROR | TWAI_ALERT_RX_DATA | TWAI_ALERT_RX_QUEUE_FULL;
  if (twai_reconfigure_alerts(alerts_to_enable, NULL) == ESP_OK) {
    Serial.println("CAN Alerts reconfigured");
  } else {
    Serial.println("Failed to reconfigure alerts");
    return;
  }

  // Init IMU
  Wire.begin(6, 7);
  while (imu.beginI2C(i2cAddress) != BMI2_OK) {
    Serial.println(
        "Error: BMI270 not connected, check wiring and I2C address!");
    delay(1000);
  }
  Serial.println("BMI270 connected!");
}

void loop() {
  imu.getSensorData();

  // Log CAN Alerts
  uint32_t alerts_triggered;
  twai_read_alerts(&alerts_triggered, pdMS_TO_TICKS(1000));
  twai_status_info_t twaistatus;
  twai_get_status_info(&twaistatus);

  if (alerts_triggered & TWAI_ALERT_ERR_PASS) {
    Serial.println("Alert: TWAI controller has become error passive.");
  }
  if (alerts_triggered & TWAI_ALERT_BUS_ERROR) {
    Serial.println(
        "Alert: A (Bit, Stuff, CRC, Form, ACK) error has occurred on the bus.");
    Serial.printf("Bus error count: %" PRIu32 "\n", twaistatus.bus_error_count);
  }
  if (alerts_triggered & TWAI_ALERT_TX_FAILED) {
    Serial.println("Alert: The Transmission failed.");
    Serial.printf("TX buffered: %" PRIu32 "\t", twaistatus.msgs_to_tx);
    Serial.printf("TX error: %" PRIu32 "\t", twaistatus.tx_error_counter);
    Serial.printf("TX failed: %" PRIu32 "\n", twaistatus.tx_failed_count);
  }
  if (alerts_triggered & TWAI_ALERT_TX_SUCCESS) {
    Serial.println("Alert: The Transmission was successful.");
    Serial.printf("TX buffered: %" PRIu32 "\t", twaistatus.msgs_to_tx);
  }

  // Check if message is received
  if (alerts_triggered & TWAI_ALERT_RX_DATA) {
    twai_message_t message;
    while (twai_receive(&message, 0) == ESP_OK) {
      parse_rx(message);
    }
  }

  // IMU Serial Data Output
  Serial.print("Accel (g) X:");
  Serial.print(imu.data.accelX, 3);
  Serial.print(" Y:");
  Serial.print(imu.data.accelY, 3);
  Serial.print(" Z:");
  Serial.print(imu.data.accelZ, 3);
  Serial.print("  Gyro (deg/s) X:");
  Serial.print(imu.data.gyroX, 3);
  Serial.print(" Y:");
  Serial.print(imu.data.gyroY, 3);
  Serial.print(" Z:");
  Serial.println(imu.data.gyroZ, 3);

  // Sending IMU Data Over CAN
  uint8_t accelFrame[8];
  packFloat(accelFrame, 0, imu.data.accelX, ACCEL_SCALE);
  packFloat(accelFrame, 2, imu.data.accelY, ACCEL_SCALE);
  packFloat(accelFrame, 4, imu.data.accelZ, ACCEL_SCALE);
  packFloat(accelFrame, 6, 0, ACCEL_SCALE);
  sendCAN(CAN_ID_ACCEL, accelFrame, 8);

  uint8_t gyroFrame[8];
  packFloat(gyroFrame, 0, imu.data.gyroX, GYRO_SCALE);
  packFloat(gyroFrame, 2, imu.data.gyroY, GYRO_SCALE);
  packFloat(gyroFrame, 4, imu.data.gyroZ, GYRO_SCALE);
  packFloat(gyroFrame, 6, 0, GYRO_SCALE);
  sendCAN(CAN_ID_GYRO, gyroFrame, 8);

  delay(20); // 50 Hz
}