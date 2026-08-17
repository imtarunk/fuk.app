import type { Config } from "tailwindcss";

export default {
  content: [
    "./pill.html",
    "./settings.html",
    "./src/**/*.{ts,tsx,html}",
  ],
  theme: {
    extend: {
      colors: {
        paper: {
          DEFAULT: "#f5f1e8",
          deep: "#ece6d6",
        },
        card: "#fffdf6",
        ink: {
          DEFAULT: "#171310",
          soft: "#443f38",
        },
        mute: "#6f695e",
        faint: "#a29a89",
        line: "#d9d2c0",
        red: {
          DEFAULT: "#e0361a",
          deep: "#b1290f",
        },
        green: "#2b7a3f",
        amber: "#a87a00",
        rec: "#e0361a",
      },
      fontFamily: {
        sans: ["Inter", "ui-sans-serif", "system-ui", "sans-serif"],
        display: ["Space Grotesk", "ui-sans-serif", "system-ui", "sans-serif"],
        serif: ["Instrument Serif", "ui-serif", "Georgia", "serif"],
        mono: ["JetBrains Mono", "ui-monospace", "SFMono-Regular", "monospace"],
      },
      boxShadow: {
        "press-sm": "2px 2px 0 0 #171310",
        press: "3px 3px 0 0 #171310",
        "press-lg": "8px 8px 0 0 #171310",
        keycap: "0 2px 0 0 #171310",
      },
      keyframes: {
        "press-hover": {
          to: { transform: "translate(2px, 2px)", boxShadow: "1px 1px 0 0 #171310" },
        },
        wave: {
          "0%, 100%": { transform: "scaleY(0.35)" },
          "50%": { transform: "scaleY(1)" },
        },
        "caret-blink": {
          "0%, 100%": { opacity: "1" },
          "50%": { opacity: "0" },
        },
        breathe: {
          "0%, 100%": { opacity: "0.55", transform: "scale(0.85)" },
          "50%": { opacity: "1", transform: "scale(1)" },
        },
        pillIn: {
          from: { opacity: "0", transform: "translateY(3px)" },
          to: { opacity: "1", transform: "translateY(0)" },
        },
        pillOut: {
          from: { opacity: "1" },
          to: { opacity: "0" },
        },
        popIn: {
          "0%": { opacity: "0" },
          "100%": { opacity: "1" },
        },
      },
      animation: {
        wave: "wave 0.9s ease-in-out infinite",
        "caret-blink": "caret-blink 1s step-end infinite",
        breathe: "breathe 1.6s ease-in-out infinite",
        pillIn: "pillIn 120ms ease both",
        pillOut: "pillOut 120ms ease both",
        popIn: "popIn 120ms ease both",
      },
      transitionDuration: {
        press: "120ms",
      },
    },
  },
  plugins: [],
} satisfies Config;
