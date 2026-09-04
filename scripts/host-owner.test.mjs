import assert from "node:assert/strict";
import test from "node:test";
import { frame, Frames, FRAME_LIMIT } from "../bin/host-owner.js";

test("owner frames decode bytewise and coalesced without losing boundaries", () => {
  const values = [{ version: 1, kind: "owned", text: "你好" }, { kind: "terminal" }];
  const bytes = Buffer.concat(values.map(frame));
  for (const size of [1, 3, bytes.length]) {
    const seen = [];
    const frames = new Frames(value => seen.push(value));
    for (let offset = 0; offset < bytes.length; offset += size) frames.push(bytes.subarray(offset, offset + size));
    frames.end();
    assert.deepEqual(seen, values);
  }
});

test("owner frame bounds reject bad lengths before allocating payloads", () => {
  for (const length of [0, FRAME_LIMIT + 1, 0xffffffff]) {
    const header = Buffer.alloc(4);
    header.writeUInt32BE(length);
    assert.throws(() => new Frames(() => {}).push(header), /length/);
  }
  assert.throws(() => frame({ text: "x".repeat(FRAME_LIMIT) }), /length/);
});

test("owner frames reject truncation, malformed JSON, and invalid UTF-8", () => {
  const bytes = frame({ ok: true });
  for (const length of [1, 3, 5, bytes.length - 1]) {
    const frames = new Frames(() => {});
    frames.push(bytes.subarray(0, length));
    assert.throws(() => frames.end(), /truncated/);
  }
  for (const payload of [Buffer.from("{"), Buffer.from([0xff]), Buffer.from('{"version":1,"version":2}'), Buffer.from('{"version":1,"\\u0076ersion":2}')]) {
    const header = Buffer.alloc(4); header.writeUInt32BE(payload.length);
    assert.throws(() => new Frames(() => {}).push(Buffer.concat([header, payload])));
  }
});
