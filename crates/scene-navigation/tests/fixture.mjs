// Independently generated FO3/NV NiTriStrips scene. No game resources needed.
export function navigationNif() {
  const bytes = [];
  const u8 = (v) => bytes.push(v);
  const u16 = (v) => bytes.push(v & 255, (v >>> 8) & 255);
  const u32 = (v) => bytes.push(v & 255, (v >>> 8) & 255, (v >>> 16) & 255, (v >>> 24) & 255);
  const f32 = (v) => { const a = new Uint8Array(4); new DataView(a.buffer).setFloat32(0, v, true); bytes.push(...a); };
  const text = (s) => { const b = new TextEncoder().encode(s); u32(b.length); bytes.push(...b); };
  const blocks = [];
  const positions = [[-3, 0, 0], [2, 0, 1], [0, 2, 4]];
  for (let index = 0; index < positions.length; index++) {
    bytes.length = 0;
    u32(index); u32(0); u32(0xffffffff); u32(14);
    positions[index].forEach(f32);
    [1, 0, 0, 0, 1, 0, 0, 0, 1].forEach(f32); f32(1);
    u32(0); u32(0xffffffff); u32(index * 2 + 1); u32(0xffffffff); u32(0); u32(0xffffffff); u8(0);
    blocks.push([...bytes]); bytes.length = 0;
    u32(0); u16(4); bytes.push(0, 0, 1);
    // Vertical quad, Z-up in NIF, facing the initial viewer.
    [-1, 0, -1, 1, 0, -1, -1, 0, 1, 1, 0, 1].forEach(f32);
    u16(1); u8(1);
    for (let i = 0; i < 4; i++) [0, -1, 0].forEach(f32);
    [0, 0, 0, 2].forEach(f32); u8(1);
    for (let i = 0; i < 4; i++) [index === 0 ? 1 : 0.2, index === 1 ? 1 : 0.2, index === 2 ? 1 : 0.2, 1].forEach(f32);
    [0, 0, 1, 0, 0, 1, 1, 1].forEach(f32);
    u16(0); u32(0xffffffff); u16(2); u16(1); u16(4); u8(1); [0, 1, 2, 3].forEach(u16);
    blocks.push([...bytes]);
  }
  bytes.length = 0;
  bytes.push(...new TextEncoder().encode("Gamebryo File Format, Version 20.2.0.7\n"));
  u32(0x14020007); u8(1); u32(11); u32(blocks.length); u32(34); bytes.push(1, 0, 1, 0, 1, 0);
  u16(2); text("NiTriStrips"); text("NiTriStripsData");
  blocks.forEach((_, i) => u16(i % 2)); blocks.forEach((b) => u32(b.length));
  u32(3); u32(8); ["left", "right", "above"].forEach(text); u32(0);
  blocks.forEach((b) => bytes.push(...b)); u32(3); [0, 2, 4].forEach(u32);
  return bytes;
}
