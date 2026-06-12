const fs = require("fs");

const SR = 44100, DUR = 16, NS = SR * DUR;
const chords = [
  { r: 261.63, t: 329.63, f: 392.0, s: 493.88 },
  { r: 392.0,  t: 493.88, f: 587.33, s: 0 },
  { r: 440.0,  t: 523.25, f: 659.25, s: 783.99 },
  { r: 349.23, t: 440.0, f: 523.25, s: 698.46 },
];
const CD = SR / 4;

function getChord(t) {
  return chords[Math.floor(t / CD) % 4];
}

const samples = [];
for (let i = 0; i < NS; i++) {
  const t = i / SR, c = getChord(i);
  let s = 0;
  s += Math.sin(2 * Math.PI * c.r * t) * 0.12;
  s += Math.sin(2 * Math.PI * c.t * t) * 0.08;
  s += Math.sin(2 * Math.PI * c.f * t) * 0.06;
  if (c.s > 0) s += Math.sin(2 * Math.PI * c.s * t) * 0.04;
  s += Math.sin(2 * Math.PI * (c.r / 2) * t) * 0.15;
  const arp = [c.r, c.t, c.f, c.r * 2];
  s += Math.sin(2 * Math.PI * arp[Math.floor(t * 6) % 4] * t) * 0.06;
  const bp = (t * 2) % 1;
  if (bp < 0.1) s += Math.sin(Math.PI * bp / 0.1) * 0.08;
  if (t < 0.5) s *= t / 0.5;
  if (t > DUR - 1) s *= (DUR - t) / 1;
  s = Math.max(-1, Math.min(1, s));
  samples.push(s, s);
}

const h = Buffer.alloc(44);
h.write("RIFF", 0);
h.writeUInt32LE(36 + samples.length * 2, 4);
h.write("WAVE", 8);
h.write("fmt ", 12);
h.writeUInt32LE(16, 16);
h.writeUInt16LE(1, 20);
h.writeUInt16LE(2, 22);
h.writeUInt32LE(SR, 24);
h.writeUInt32LE(SR * 4, 28);
h.writeUInt16LE(4, 32);
h.writeUInt16LE(16, 34);
h.write("data", 36);
h.writeUInt32LE(samples.length * 2, 40);

const d = Buffer.alloc(samples.length * 2);
for (let i = 0; i < samples.length; i++) {
  d.writeInt16LE(Math.floor(samples[i] * 32767), i * 2);
}

const out = "C:\\Users\\Administrator\\Desktop\\临时\\pointer-intro\\public\\bgmusic.wav";
fs.writeFileSync(out, Buffer.concat([h, d]));
console.log("OK:", out, (Buffer.concat([h, d]).length / 1024 / 1024).toFixed(2) + "MB");
