/**
 * AIRCP DDS Helpers -- CDR2 encode/decode for Studio <-> daemon communication
 */
import { hdds } from './hdds-client.js';
import { TOPIC_COMMANDS } from './topics';
import { Cdr2Buffer, aircp } from './aircp_generated';

/**
 * Decode a DDS sample and extract the payload data.
 * CDR2-decodes _raw and parses payload_json.
 */
export function unwrapPayload(sample: any): any {
  if (sample && sample._raw && typeof sample._raw === 'string') {
    try {
      const binary = atob(sample._raw);
      const bytes = new Uint8Array(binary.length);
      for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
      const buf = new Cdr2Buffer(bytes);
      const msg = aircp.decodeMessage(buf);
      if (msg.payload_json) {
        return JSON.parse(msg.payload_json);
      }
    } catch { /* fall through */ }
  }
  return sample;
}

/**
 * Publish a command to the daemon via DDS (CDR2-encoded AIRCP Message).
 */
export function publishCommand(command: string, params: Record<string, any> = {}, operatorId: string = '@operator'): boolean {
  const msg: aircp.Message = {
    id: crypto.randomUUID(),
    room: 'commands',
    from_id: operatorId,
    from_type: aircp.SenderType.USER,
    kind: aircp.MessageKind.CONTROL,
    payload_json: JSON.stringify({ command, ...params }),
    timestamp_ns: BigInt(Date.now()) * 1000000n,
    protocol_version: '0.2.0',
    broadcast: true,
    to_agent_id: '',
    room_seq: 0n,
    project: '',
  };

  const buf = new Cdr2Buffer(new ArrayBuffer(8192));
  aircp.encodeMessage(msg, buf);
  const bytes = buf.toBytes();

  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }

  return hdds.publish(TOPIC_COMMANDS, { _raw: btoa(binary) });
}
