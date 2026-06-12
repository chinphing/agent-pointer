// bgm.js - generate WAV background music
const fs = require("fs");
const path = require("path");

const SR = 44100; // sample rate
const DUR = 16;   // seconds
const NS = SR * DUR;

const chords = [
  { r: 261.63, t: 329.63, f: 392.0, s: 493.88 },  // Cmaj7
  { r: 392.0,  t: 493.88, f: 587.33, s: 0 },       // G
  { r: 440.0,  t: 523.25, f: 659.25, s: 783.99 },  // Am7
  { r: 349.23, t: 440.0,  f: 523.25, s: 698.46 },  // Fmaj7
];
const CD = SR / 4; // chord duration (0.25s each)

function getChord(t) {
  return chords[Math.floor(t / CD) % 4];
}

console.log("Generating audio...");
const samples = [];

for (let i = 0; i < NS; i++) {
  const t = i / SR;
  const c = getChord(i);

  let s = 0;
  // Pad
  s += Math.sin(2 * Math.PI * c.r * t) * 0.12;
  s += Math.sin(2 * Math.PI * c.t * t) * 0.08;
  s += Math.sin(2 * Math.PI * c.f * t) * 0.06;
  if (c.s > 0) s += Math.sin(2 * Math.PI * c.s * t) * 0.04;
  // Bass
  s += Math.sin(2 * Math.PI * (c.r / 2) * t) * 0.15;
  // Arp
  const arpNotes = [c.r, c.t, c.f, c.r * 2];
  const arpIdx = Math.floor(t * 6) % 4;
  s += Math.sin(2 * Math.PI * arpNotes[arpIdx] * t) * 0.06;
  // Pulse
  const bp = (t * 2) % 1;
  if (bp < 0.1) s += Math.sin(Math.PI * bp / 0.1) * 0.08;
  // Fade
  if (t < 0.5) s *= t / 0.5;
  if (t > DUR - 1) s *= (DUR - t) / 1;

  s = Math.max(-1, Math.min(1, s));
  samples.push(s, s); // stereo
}

// Build WAV
const header = Buffer.alloc(44);
header.write("RIFF", 0);
header.writeUInt32LE(36 + samples.length * 2, 4);
header.write("WAVE", 8);
header.write("fmt ", 12);
header.writeUInt32LE(16, 16);
header.writeUInt16LE(1, 20);    // PCM
header.writeUInt16LE(2, 22);    // stereo
header.writeUInt32LE(SR, 24);
header.writeUInt32LE(SR * 4, 28); // byte rate
header.writeUInt16LE(4, 32);    // block align
header.writeUInt16LE(16, 34);   // bits per sample
header.write("data", 36);
header.writeUInt32LE(samples.length * 2, 40);

const data = Buffer.alloc(samples.length * 2);
for (let i = 0; i < samples.length; i++) {
  data.writeInt16LE(Math.floor(samples[i] * 32767), i * 2);
}

const outPath = path.join(__dirname, "public", "bgmusic.wav");
fs.writeFileSync(outPath, Buffer.concat([header, data]));
console.log("Done! File:", outPath);
console.log("Size:", (header.length + data.length / 1024 / 1024).toFixed(2), "MB");
