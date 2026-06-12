const https = require("https");
const fs = require("fs");

function fetch(url) {
  return new Promise((resolve, reject) => {
    https.get(url, { headers: { "User-Agent": "Mozilla/5.0" }, timeout: 30000 }, (res) => {
      let d = "";
      res.on("data", (c) => (d += c));
      res.on("end", () => resolve(d));
    }).on("error", reject);
  });
}

async function main() {
  const page = await fetch("https://pixabay.com/music/corporate-corporate-technology-dreamer-112176/");
  
  // Find all "downloadUrl" patterns
  const patterns = [
    /https?:[^\s"']+download[^\s"']+\.mp3[^\s"']*/gi,
    /downloadUrl["'\s:=]+["']([^"']+)["']/gi,
    /cdn\.pixabay\.com[^\s"']+/gi,
  ];
  
  for (const p of patterns) {
    const matches = [...page.matchAll(p)];
    if (matches.length > 0) {
      for (const m of matches) {
        const url = (m[1] || m[0]).replace(/\\u002F/g, "/");
        if (url.includes(".mp3") || url.includes("download")) {
          console.log("MATCH:", url.replace(/^https?:\/\//, "https://").replace(/\/\//g, "/"));
        }
      }
    }
  }
  
  // Also dump all script/data tags that might contain URLs
  const scriptMatch = page.match(/<script[^>]*>([^<]*)<\/script>/gi);
  if (scriptMatch) {
    for (const s of scriptMatch.slice(0, 5)) {
      const m2 = s.match(/https?:\/\/cdn\.pixabay[^"']+/);
      if (m2) console.log("SCRIPT:", m2[0].replace(/\\u002F/g, "/"));
    }
  }
  
  console.log("Page length:", page.length);
}

main().catch((e) => console.log("ERR:", e.message));
