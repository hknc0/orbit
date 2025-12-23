// WebTransport connection handler for game networking

import type { ClientMessage, ServerMessage, PlayerInput } from './Protocol';
import { encodeClientMessage, decodeServerMessage } from './Codec';

export type ConnectionState = 'disconnected' | 'connecting' | 'connected' | 'error';

export interface TransportEvents {
  onStateChange: (state: ConnectionState) => void;
  onMessage: (message: ServerMessage) => void;
  onError: (error: Error) => void;
}

export class GameTransport {
  private transport: WebTransport | null = null;
  private reliableWriter: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private datagramWriter: WritableStreamDefaultWriter<Uint8Array> | null = null;
  private state: ConnectionState = 'disconnected';
  private events: TransportEvents;
  private pingInterval: number | null = null;
  private lastPingTime: number = 0;
  private rtt: number = 0;

  constructor(events: TransportEvents) {
    this.events = events;
  }

  async connect(url: string, certHash?: string): Promise<void> {
    if (this.state === 'connecting' || this.state === 'connected') {
      return;
    }

    this.setState('connecting');

    try {
      const options: WebTransportOptions = {};

      // For development with self-signed certs
      if (certHash) {
        options.serverCertificateHashes = [
          {
            algorithm: 'sha-256',
            value: this.base64ToArrayBuffer(certHash),
          },
        ];
      }

      this.transport = new WebTransport(url, options);
      await this.transport.ready;

      // Set up reliable bidirectional stream
      const stream = await this.transport.createBidirectionalStream();
      this.reliableWriter = stream.writable.getWriter();

      // Set up datagram channel for unreliable input
      this.datagramWriter = this.transport.datagrams.writable.getWriter();

      // Start reading messages
      this.startReading(stream.readable);
      this.startReadingDatagrams();

      this.setState('connected');

      // Start ping interval
      this.startPingInterval();

      // Handle connection close
      this.transport.closed
        .then(() => {
          this.handleDisconnect();
        })
        .catch((err) => {
          this.handleError(err);
        });
    } catch (err) {
      this.handleError(err instanceof Error ? err : new Error(String(err)));
    }
  }

  disconnect(): void {
    this.stopPingInterval();

    if (this.reliableWriter) {
      this.reliableWriter.close().catch(() => {});
      this.reliableWriter = null;
    }

    if (this.datagramWriter) {
      this.datagramWriter.close().catch(() => {});
      this.datagramWriter = null;
    }

    if (this.transport) {
      this.transport.close();
      this.transport = null;
    }

    this.setState('disconnected');
  }

  async sendReliable(message: ClientMessage): Promise<void> {
    if (!this.reliableWriter || this.state !== 'connected') {
      throw new Error('Not connected');
    }

    const data = encodeClientMessage(message);
    // Prefix with length for framing
    const framed = new Uint8Array(4 + data.length);
    new DataView(framed.buffer).setUint32(0, data.length, true);
    framed.set(data, 4);

    await this.reliableWriter.write(framed);
  }

  sendUnreliable(input: PlayerInput): void {
    if (!this.datagramWriter || this.state !== 'connected') {
      return; // Silently drop if not connected
    }

    const message: ClientMessage = { type: 'Input', input };
    const data = encodeClientMessage(message);

    this.datagramWriter.write(data).catch(() => {
      // Datagram may fail silently
    });
  }

  async sendPing(): Promise<void> {
    this.lastPingTime = performance.now();
    await this.sendReliable({
      type: 'Ping',
      timestamp: Date.now(),
    });
  }

  getRtt(): number {
    return this.rtt;
  }

  getState(): ConnectionState {
    return this.state;
  }

  private setState(state: ConnectionState): void {
    if (this.state !== state) {
      this.state = state;
      this.events.onStateChange(state);
    }
  }

  private handleError(error: Error): void {
    this.setState('error');
    this.events.onError(error);
    this.disconnect();
  }

  private handleDisconnect(): void {
    this.stopPingInterval();
    this.reliableWriter = null;
    this.datagramWriter = null;
    this.transport = null;
    this.setState('disconnected');
  }

  private async startReading(readable: ReadableStream<Uint8Array>): Promise<void> {
    const reader = readable.getReader();
    let buffer = new Uint8Array(0);

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        // Append to buffer
        const newBuffer = new Uint8Array(buffer.length + value.length);
        newBuffer.set(buffer);
        newBuffer.set(value, buffer.length);
        buffer = newBuffer;

        // Process complete messages
        while (buffer.length >= 4) {
          const msgLength = new DataView(buffer.buffer, buffer.byteOffset).getUint32(0, true);
          if (buffer.length < 4 + msgLength) break;

          // Create a proper copy of the message data (slice shares the underlying buffer)
          const msgData = buffer.slice(4, 4 + msgLength);
          const msgBuffer = msgData.buffer.slice(msgData.byteOffset, msgData.byteOffset + msgData.byteLength);
          buffer = buffer.slice(4 + msgLength);

          try {
            const message = decodeServerMessage(msgBuffer);
            this.handleMessage(message);
          } catch (err) {
            console.error('Failed to decode message:', err);
          }
        }
      }
    } catch (err) {
      if (this.state === 'connected') {
        this.handleError(err instanceof Error ? err : new Error(String(err)));
      }
    }
  }

  private async startReadingDatagrams(): Promise<void> {
    if (!this.transport) return;

    const reader = this.transport.datagrams.readable.getReader();

    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        try {
          // Create a proper copy of the buffer
          const msgBuffer = value.buffer.slice(value.byteOffset, value.byteOffset + value.byteLength);
          const message = decodeServerMessage(msgBuffer);
          this.handleMessage(message);
        } catch (err) {
          // Datagram decode errors are expected occasionally
        }
      }
    } catch (err) {
      // Datagram reading may stop on disconnect
    }
  }

  private handleMessage(message: ServerMessage): void {
    // Handle pong for RTT calculation
    if (message.type === 'Pong') {
      this.rtt = performance.now() - this.lastPingTime;
    }

    this.events.onMessage(message);
  }

  private startPingInterval(): void {
    this.pingInterval = window.setInterval(() => {
      this.sendPing().catch(() => {});
    }, 1000);
  }

  private stopPingInterval(): void {
    if (this.pingInterval !== null) {
      clearInterval(this.pingInterval);
      this.pingInterval = null;
    }
  }

  private base64ToArrayBuffer(base64: string): ArrayBuffer {
    const binary = atob(base64);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i++) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes.buffer;
  }
}
