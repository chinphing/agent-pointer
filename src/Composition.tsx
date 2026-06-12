import React from "react";
import { AbsoluteFill, interpolate, useCurrentFrame, useVideoConfig, staticFile } from "remotion";
import { Audio } from "@remotion/media";
import { TransitionSeries, linearTiming } from "@remotion/transitions";
import { fade } from "@remotion/transitions/fade";

const COLORS = {
  bg: "#0a0a23",
  primary: "#00d4ff",
  white: "#ffffff",
  gray: "#8892b0",
  lightBg: "#12123a",
};

const AnimatedText: React.FC<{
  children: React.ReactNode;
  style?: React.CSSProperties;
  delay?: number;
}> = ({ children, style, delay = 0 }) => {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame - delay, [0, 25], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: (t) => t * (2 - t),
  });
  const slideY = interpolate(frame - delay, [0, 25], [30, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return (
    <div style={{ opacity, transform: `translateY(${slideY}px)`, ...style }}>
      {children}
    </div>
  );
};

const SceneLogo: React.FC = () => {
  const f = useCurrentFrame();
  const scale = interpolate(f, [0, 35], [0.6, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
    easing: (t) => 1 - Math.pow(1 - t, 3),
  });
  const logoO = interpolate(f, [0, 30], [0, 1], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  const glow = interpolate(f, [0, 30, 60, 90], [0, 0.4, 0.4, 0], {
    extrapolateLeft: "clamp",
    extrapolateRight: "clamp",
  });
  return (
    <AbsoluteFill style={{ backgroundColor: COLORS.bg, display: "flex", alignItems: "center", justifyContent: "center" }}>
      <div style={{ position: "absolute", width: 300, height: 300, borderRadius: "50%", background: `radial-gradient(circle, rgba(0,212,255,${glow}) 0%, transparent 70%)` }} />
      <div style={{ opacity: logoO, transform: `scale(${scale})`, textAlign: "center" }}>
        <div style={{ width: 100, height: 100, background: "linear-gradient(135deg, #00d4ff, #0066ff)", borderRadius: 24, margin: "0 auto 28px", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 52, color: COLORS.white, fontWeight: 800, boxShadow: "0 0 60px rgba(0,212,255,0.3)" }}>P</div>
        <h1 style={{ fontSize: 96, color: COLORS.white, margin: 0, letterSpacing: 6, fontWeight: 800 }}>Pointer</h1>
      </div>
      <AnimatedText delay={35} style={{ marginTop: 24 }}>
        <p style={{ fontSize: 28, color: COLORS.primary, letterSpacing: 10, textTransform: "uppercase", margin: 0, fontWeight: 600 }}>AI Agent Platform</p>
      </AnimatedText>
    </AbsoluteFill>
  );
};

const SceneWhat: React.FC = () => (
  <AbsoluteFill style={{ backgroundColor: COLORS.bg, display: "flex", alignItems: "center", justifyContent: "center", padding: 60 }}>
    <AnimatedText delay={0}><h2 style={{ fontSize: 52, color: COLORS.white, fontWeight: 700, margin: "0 0 30px 0", textAlign: "center" }}>什么是 Pointer？</h2></AnimatedText>
    <AnimatedText delay={20}>
      <p style={{ fontSize: 26, color: COLORS.gray, textAlign: "center", lineHeight: 1.7, maxWidth: 900, margin: 0 }}>
        Pointer 是一个强大的 <span style={{ color: COLORS.primary }}>AI 智能体平台</span>，集成了 Claude、GPT 等主流大模型，支持多种 IM 工具和桌面端使用。
      </p>
    </AnimatedText>
    <AnimatedText delay={40}>
      <p style={{ fontSize: 22, color: COLORS.gray, textAlign: "center", marginTop: 40, opacity: 0.7 }}>让 AI 成为你真正的生产力伙伴</p>
    </AnimatedText>
  </AbsoluteFill>
);

const features = [
  { icon: "🔌", text: "IM 集成 — 微信 / 飞书 / 企微 / 钉钉" },
  { icon: "🧠", text: "多模型支持 — Claude / GPT / 开源模型" },
  { icon: "🎯", text: "自定义 Skills — 按需扩展智能体能力" },
  { icon: "💻", text: "全平台 — macOS / Windows / Linux" },
  { icon: "🔒", text: "本地数据优先，隐私可控" },
];

const SceneFeatures: React.FC = () => (
  <AbsoluteFill style={{ backgroundColor: COLORS.bg, display: "flex", alignItems: "center", justifyContent: "center", padding: 60 }}>
    <AnimatedText delay={0}><h2 style={{ fontSize: 48, color: COLORS.white, fontWeight: 700, margin: "0 0 50px 0", textAlign: "center" }}>核心功能</h2></AnimatedText>
    <div style={{ display: "flex", flexDirection: "column", gap: 20, maxWidth: 700 }}>
      {features.map((f, i) => (
        <AnimatedText key={i} delay={15 + i * 15}>
          <div style={{ display: "flex", alignItems: "center", gap: 16, padding: "14px 24px", borderRadius: 12, background: COLORS.lightBg, border: "1px solid rgba(0,212,255,0.08)" }}>
            <span style={{ fontSize: 28 }}>{f.icon}</span>
            <span style={{ fontSize: 22, color: COLORS.white, fontWeight: 500 }}>{f.text}</span>
          </div>
        </AnimatedText>
      ))}
    </div>
  </AbsoluteFill>
);

const SceneTagline: React.FC = () => (
  <AbsoluteFill style={{ backgroundColor: COLORS.bg, display: "flex", alignItems: "center", justifyContent: "center", padding: 60 }}>
    <AnimatedText delay={0}>
      <h2 style={{ fontSize: 56, color: COLORS.white, fontWeight: 700, textAlign: "center", lineHeight: 1.4, margin: 0 }}>
        AI 驱动，<br /><span style={{ color: COLORS.primary }}>无限可能</span>
      </h2>
    </AnimatedText>
    <AnimatedText delay={25} style={{ marginTop: 40 }}>
      <p style={{ fontSize: 24, color: COLORS.gray, textAlign: "center", maxWidth: 700, margin: 0, lineHeight: 1.6 }}>
        从对话到自动化，从个人助手到企业工作流<br />Pointer 为你连接一切
      </p>
    </AnimatedText>
  </AbsoluteFill>
);

const SceneEnd: React.FC = () => {
  const f = useCurrentFrame();
  const fadeOut = interpolate(f, [60, 90], [1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  return (
    <AbsoluteFill style={{ backgroundColor: COLORS.bg, display: "flex", alignItems: "center", justifyContent: "center", opacity: fadeOut }}>
      <div style={{ textAlign: "center" }}>
        <div style={{ width: 80, height: 80, background: "linear-gradient(135deg, #00d4ff, #0066ff)", borderRadius: 20, margin: "0 auto 24px", display: "flex", alignItems: "center", justifyContent: "center", fontSize: 40, color: COLORS.white, fontWeight: 800, boxShadow: "0 0 40px rgba(0,212,255,0.3)" }}>P</div>
        <h2 style={{ fontSize: 48, color: COLORS.white, fontWeight: 700, margin: "0 0 16px 0", letterSpacing: 3 }}>Pointer</h2>
        <p style={{ fontSize: 22, color: COLORS.primary, letterSpacing: 6, textTransform: "uppercase", margin: 0 }}>AI Agent Platform</p>
      </div>
    </AbsoluteFill>
  );
};

export const MyComposition: React.FC = () => {
  const { fps } = useVideoConfig();
  const td = 15;
  return (
    <>
      <Audio src={staticFile("bgmusic.wav")} volume={0.7} />
      <TransitionSeries>
        <TransitionSeries.Sequence durationInFrames={3 * fps}>
          <SceneLogo />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={linearTiming({ durationInFrames: td })} />
        <TransitionSeries.Sequence durationInFrames={3 * fps}>
          <SceneWhat />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={linearTiming({ durationInFrames: td })} />
        <TransitionSeries.Sequence durationInFrames={4 * fps}>
          <SceneFeatures />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={linearTiming({ durationInFrames: td })} />
        <TransitionSeries.Sequence durationInFrames={3 * fps}>
          <SceneTagline />
        </TransitionSeries.Sequence>
        <TransitionSeries.Transition presentation={fade()} timing={linearTiming({ durationInFrames: td })} />
        <TransitionSeries.Sequence durationInFrames={3 * fps}>
          <SceneEnd />
        </TransitionSeries.Sequence>
      </TransitionSeries>
    </>
  );
};
