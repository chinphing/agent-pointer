// Generate a simple corporate/tech-style background music WAV file
const fs = require("fs");

const SAMPLE_RATE = 44100;
const DURATION_SEC = 16; // slightly longer than video
const NUM_SAMPLES = SAMPLE_RATE * DURATION_SEC;
const NUM_CHANNELS = 2;
const BITS_PER_SAMPLE = 16;
const BYTES_PER_SAMPLE = BITS_PER_SAMPLE / 8;

// Chord progression: Cmaj7 - G - Am - F (inspiring/tech feel)
// Frequencies (root, 3rd, 5th, 7th for chords)
const chords = [
  // Cmaj7
  { root: 261.63, third: 329.63, fifth: 392.0, seventh: 493.88 },
  // G
  { root: 392.0, third: 493.88, fifth: 587.33, seventh: 0 },
  // Am7
  { root: 440.0, third: 523.25, fifth: 659.25, seventh: 783.99 },
  // Fmaj7
  { root: 349.23, third: 440.0, fifth: 523.25, seventh: 698.46 },
];

const chordDuration = SAMPLE_RATE / 4; // 0.25 sec per chord, loop

function getChordPosition(t) {
  const chordIndex = Math.floor(t / chordDuration) % chords.length;
  return chords[chordIndex];
}

// Write WAV header
function writeWAV(data, filePath) {
  const dataSize = data.length * BYTES_PER_SAMPLE;
  const header = Buffer.alloc(44);
  
  // RIFF header
  header.write("RIFF", 0);
  header.writeUInt32LE(36 + dataSize, 4);
  header.write("WAVE", 8);
  
  // fmt chunk
  header.write("fmt ", 12);
  header.writeUInt32LE(16, 16); // chunk size
  header.writeUInt16LE(1, 20); // PCM
  header.writeUInt16LE(NUM_CHANNELS, 22);
  header.writeUInt32LE(SAMPLE_RATE, 24);
  header.writeUInt32LE(SAMPLE_RATE * NUM_CHANNELS * BYTES_PER_SAMPLE, 28); // byte rate
  header.writeUInt16LE(NUM_CHANNELS * BYTES_PER_SAMPLE, 32); // block align
  header.writeUInt16LE(BITS_PER_SAMPLE, 34);
  
  // data chunk
  header.write("data", 36);
  header.writeUInt32LE(dataSize, 40);
  
  fs.writeFileSync(filePath, Buffer.concat([header, data]));
}

console.log("Generating background music...");

const samples = [];
for (let i = 0; i < NUM_SAMPLES; i++) {
  const t = i / SAMPLE_RATE;
  const ch = getChordPosition(i);
  
  // Pad sound (soft main chord)
  const padVolume = 0.12;
  const pad1 = Math.sin(2 * Math.PI * ch.root * t) * padVolume;
  const pad3 = Math.sin(2 * Math.PI * ch.third * t) * padVolume * 0.7;
  const pad5 = Math.sin(2 * Math.PI * ch.fifth * t) * padVolume * 0.5;
  const pad7 = ch.seventh > 0 ? Math.sin(2 * Math.PI * ch.seventh * t) * padVolume * 0.3 : 0;
  
  // Bass (subby sine, one octave lower)
  const bassVolume = 0.15;
  const bass = Math.sin(2 * Math.PI * (ch.root / 2) * t) * bassVolume;
  
  // Gentle arpeggio (fast notes)
  const arpVolume = 0.06;
  const arpNotes = [ch.root, ch.third, ch.fifth, ch.root * 2];
  const arpNoteIndex = Math.floor(t * 6) % arpNotes.length;
  const arpFreq = arpNotes[arpNoteIndex];
  const arp = Math.sin(2 * Math.PI * arpFreq * t) * arpVolume;
  
  // Soft rhythm pulse (low kick every beat)
  const beatFreq = 2; // beats per second
  const beatPhase = (t * beatFreq) % 1;
  const pulse = beatPhase < 0.1 ? Math.sin(Math.PI * beatPhase / 0.1) * 0.08 : 0;
  
  // Mix
  let sample = pad1 + pad3 + pad5 + pad7 + bass + arp + pulse;
  
  // Fade in (first 0.5 sec)
  if (t < 0.5) sample *= t / 0.5;
  
  // Fade out (last 1 sec)
  if (t > DURATION_SEC - 1) sample *= (DURATION_SEC - t) / 1;
  
  // Clamp
  sample = Math.max(-1, Math.min(1, sample));
  
  samples.push(sample);
  samples.push(sample); // stereo
}

// Convert to 16-bit PCM
const audioData = Buffer.alloc(samples.length * 2);
for (let i = 0; i < samples.length; i++) {
  const intSample = Math.floor(samples[i] * 32767);
  audioData.writeInt16LE(Math.max(-32768, Math.min(32767, intSample)), i * 2);
}

const outPath = "C:\\Users\\Administrator\\Desktop\\临时\\pointer-intro\\public\\bgmusic.wav";
writeWAV(audioData, outPath);
console.log("Done! Written to", outPath, "-", audioData.length, "bytes");
