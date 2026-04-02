import mqtt from 'mqtt';
import type { MqttClient } from 'mqtt';

let client: MqttClient | null = null;
const subscriptions = new Map<string, (data: any) => void>();

/**
 * Connect to MQTT broker via WebSocket
 */
export function connectMqtt(brokerUrl: string): void {
	if (client?.connected) {
		console.log('MQTT already connected');
		return;
	}

	const clientId = 'frontend_' + Math.random().toString(16).substring(2, 8);

	console.log(`Connecting to MQTT broker at ${brokerUrl} with client ID: ${clientId}`);

	client = mqtt.connect(brokerUrl, {
		clientId,
		clean: true,
		reconnectPeriod: 5000,
		keepalive: 60
	});

	client.on('connect', () => {
		console.log('MQTT connected');
	});

	client.on('error', (err) => {
		console.error('MQTT connection error:', err);
	});

	client.on('reconnect', () => {
		console.log('MQTT reconnecting...');
	});

	client.on('close', () => {
		console.log('MQTT connection closed');
	});

	client.on('message', (topic, message) => {
		const callback = subscriptions.get(topic);
		if (callback) {
			try {
				const data = JSON.parse(message.toString());
				callback(data);
			} catch (err) {
				console.error(`Failed to parse MQTT message on topic ${topic}:`, err);
			}
		}
	});
}

/**
 * Subscribe to a topic with a callback
 */
export function subscribeTopic(topic: string, callback: (data: any) => void): void {
	if (!client) {
		console.error('MQTT client not connected. Call connectMqtt() first.');
		return;
	}

	subscriptions.set(topic, callback);

	client.subscribe(topic, { qos: 0 }, (err) => {
		if (err) {
			console.error(`Failed to subscribe to ${topic}:`, err);
		} else {
			console.log(`Subscribed to MQTT topic: ${topic}`);
		}
	});
}

/**
 * Unsubscribe from a topic
 */
export function unsubscribeTopic(topic: string): void {
	if (!client) {
		return;
	}

	subscriptions.delete(topic);

	client.unsubscribe(topic, (err) => {
		if (err) {
			console.error(`Failed to unsubscribe from ${topic}:`, err);
		} else {
			console.log(`Unsubscribed from MQTT topic: ${topic}`);
		}
	});
}

/**
 * Disconnect from MQTT broker
 */
export function disconnect(): void {
	if (client) {
		client.end();
		client = null;
		subscriptions.clear();
		console.log('MQTT disconnected');
	}
}

/**
 * Check if MQTT client is connected
 */
export function isConnected(): boolean {
	return client?.connected ?? false;
}
