# Fuk design tokens

Paper-and-ink editorial brutalism. Warm cream canvas, ink-black structure, one vermilion accent, hard offset shadows, square corners. No gradients, no glass, no glow, no radius on chrome.

Source of truth in code: `src/app/globals.css` (`@theme`). Tailwind names map 1:1 (`bg-paper`, `text-ink`, `border-line`, `shadow-press`).

---

## Color

### Paper (surfaces)

| Token | CSS | Hex | Use |
|---|---|---|---|
| `paper` | `--color-paper` | `#f5f1e8` | Page canvas, nav, default section |
| `paper-deep` | `--color-paper-deep` | `#ece6d6` | Alternating section bands |
| `card` | `--color-card` | `#fffdf6` | Cards, buttons, receipt, logo plate |

### Ink (text + chrome)

| Token | CSS | Hex | Use |
|---|---|---|---|
| `ink` | `--color-ink` | `#171310` | Headlines, borders, shadows, primary buttons |
| `ink-soft` | `--color-ink-soft` | `#443f38` | Default body text |
| `mute` | `--color-mute` | `#6f695e` | Supporting copy, eyebrows |
| `faint` | `--color-faint` | `#a29a89` | Indices, captions, struck prices |
| `line` | `--color-line` | `#d9d2c0` | Quiet dividers, tertiary button border, dot grid |

### Accent

| Token | CSS | Hex | Use |
|---|---|---|---|
| `red` | `--color-red` | `#e0361a` | Brand, CTA, italic accent word, ticker, selection, logo stroke |
| `red-deep` | `--color-red-deep` | `#b1290f` | Pressed / hover deepen (reserved) |
| `green` | `--color-green` | `#2b7a3f` | Positive checks, success badges |
| `amber` | `--color-amber` | `#a87a00` | Warning (reserved, unused on page) |
| white | — | `#ffffff` | Text on red / green fills, selection text |

Red is the only loud color on marketing chrome. Green is semantic (yes / local / check), never decorative.

### Screen (demo window + terminal only)

Do not use these on page chrome. They exist so the product mock reads as a real dark app sitting on paper.

| Token | CSS | Hex | Use |
|---|---|---|---|
| `screen` | `--color-screen` | `#131114` | Demo / terminal fill |
| `screen-raise` | `--color-screen-raise` | `#1c1a1e` | Tabs, raised panes inside the screen |
| `screen-line` | `--color-screen-line` | `#2d2a30` | Screen hairlines |
| `screen-ink` | `--color-screen-ink` | `#f1efe8` | Primary text in the screen |
| `screen-body` | `--color-screen-body` | `#bab7ac` | Secondary screen text |
| `screen-mute` | `--color-screen-mute` | `#7d7a70` | Chrome labels in the screen |
| `code-blue` | `--color-code-blue` | `#7db2f0` | Syntax / links in the screen |
| `code-green` | `--color-code-green` | `#7ed6a2` | Syntax / prompt `$` |

Traffic-light dots in window chrome: `#ff5f57` / `#febc2e` / `#28c840`.

### Selection

```css
::selection {
  background: var(--color-red);
  color: #fff;
}
```

---

## Typography

Loaded via `next/font` in `src/app/layout.tsx`. CSS variables: `--font-inter`, `--font-space`, `--font-instrument`, `--font-jetbrains`.

| Role | Family | CSS | Weights | Use |
|---|---|---|---|---|
| Sans | Inter | `--font-sans` | 400 | Body copy only |
| Display | Space Grotesk | `--font-display` | 500, 600, 700 | Headlines, card titles, wordmark |
| Serif | Instrument Serif | `--font-serif` | 400 italic | One accent word per headline |
| Mono | JetBrains Mono | `--font-mono` | 400, 500, 700 | Buttons, eyebrows, labels, receipts, code |

Features on body: `"calt", "kern", "liga"`. Antialiased.

### Type scale

| Name | Size | Weight | Tracking | Leading | Family | Where |
|---|---|---|---|---|---|---|
| Display XL | 76 / 64 / 52px | 700 | −0.03em | 1.02 | Display | Hero `h1` |
| Display LG | 120px / 88px | 700 | −0.04em | 0.85 | Display | Pricing `$0` |
| Display MD | 48 / 36px | 600–700 | −0.02em | 1.1 | Display | Section `h2`, final CTA |
| Display SM | 30–32px | 600 | −0.01em | 1.25 | Display | Problem strip |
| Title | 18–20px | 600 | −0.01em | 1.3 | Display | Card / step titles |
| Body LG | 18px | 400 | 0 | 1.6–1.7 | Sans | Section descriptions |
| Body | 15px | 400 | 0 | 1.65 | Sans | Card body, FAQ answers |
| Mono UI | 13px | 500 | 0.08em uppercase | 1 | Mono | Buttons |
| Mono label | 11–12px | 500 | 0.12–0.16em uppercase | 1 | Mono | Eyebrows, specs, trust line |
| Serif accent | inherit | 400 italic | inherit | inherit | Serif | One word in `h1`/`h2`, in `red` |
| Pull quote | 24 / 20px | 400 italic | 0 | 1.3 | Serif | Requirements quote |

Rule: one italic serif word per headline, always in red. Never italicize a whole sentence.

---

## Shape

Chrome is square. No `rounded-*` on buttons, cards, nav, badges, or logo plates.

| Token | Value | Use |
|---|---|---|
| Radius | `0` | All marketing chrome |
| Keycap | square, 22×22 min | Keyboard glyphs |
| Icon tile | 44×44 (`size-11`) | Feature icons |
| Logo plate | height 36px (`h-9`), width auto | Nav / footer |
| Window dots | 12px circles | Demo / terminal chrome only |

The only circles on the page: recording pulse, traffic lights, and the black start-dot in the logo.

---

## Shadow (press)

Hard ink offsets. No blur, no spread glow.

| Token | CSS | Value | Use |
|---|---|---|---|
| `press-sm` | `--shadow-press-sm` | `2px 2px 0 0 ink` | Nav logo, GitHub icon, small plates |
| `press` | `--shadow-press` | `3px 3px 0 0 ink` | Primary / accent / secondary buttons |
| `press-lg` | `--shadow-press-lg` | `8px 8px 0 0 ink` | Cards, tables, demo window, receipt |
| Keycap | — | `0 2px 0 0 ink` | Physical key lip |

Hover on press elements (`press-hover`): translate `(2px, 2px)`, shadow collapses to `1px 1px 0 0 ink`. Duration `120ms ease`.

Dark CTA block (final download): shadow uses paper, `3px 3px 0 0 paper`.

---

## Layout

| Token | Value |
|---|---|
| Content width | `1200px` |
| Page gutter | `24px` (`px-6`) |
| Section padding | `96px` / `64px` lg / `48px` md |
| Header height | `64px` (`h-16`) |
| Section header gap | `48px` below (`mb-12`), `32px` on mobile |
| Grid gap (legacy cards) | none — sections share one ruled block with `border-ink` internal rules |
| Dot grid | 24×24, 1px `line` dots |

Hairlines are `1px solid ink` for structure, `1px solid line` for quiet rows. Dashed `ink` for receipts and pipeline connectors.

---

## Motion

| Name | Timing | Use |
|---|---|---|
| `press-hover` | 120ms ease | Button / plate press |
| `wave` | 0.5–1.3s ease-in-out infinite | Recording waveform bars |
| `caret-blink` | 1s step-end infinite | Carets |
| `scene-swap` | 400ms ease-out | Demo scene change |
| `marquee` | 22s linear infinite | Red ticker |
| `ping` / `pulse` | Tailwind defaults | Live recording / version badge |

No fade-up on scroll. The page is static paper; motion lives inside the product demo and the ticker.

---

## Logo

Mark: vermilion waveform from a black circle (voice) to a black square (text), on paper.

| Asset | Path | Size |
|---|---|---|
| Wide mark | `/logo.png` | 1024×512 |
| Square (icons) | `/logo-square.png` | 1024×1024 |
| Favicon | `src/app/icon.png` | 512×512 |
| Apple | `src/app/apple-icon.png` | 180×180 |

Lockup: logo plate (border ink, card fill, `press-sm`) + lowercase Space Grotesk wordmark `fuk`.

On dark footer: same plate on `card` so the cream mark stays readable.

Alt text: `{name} logo — a red waveform from voice to text`.

---

## Components

### Button

Height `44px` (`h-11`), padding `px-5`, mono 13px / 500 / uppercase / `0.08em`. Border `ink` unless noted.

| Variant | Fill | Text | Shadow |
|---|---|---|---|
| `primary` | `ink` | `paper` | `press` |
| `accent` | `red` | white | `press` |
| `secondary` | `card` | `ink` | `press` |
| `tertiary` | transparent | `ink` | none; border `line` → `ink` on hover |
| `outline` | transparent | `paper` | none; for use on `ink` blocks |

Loudest CTA on the page is `accent`. Nav Download uses `accent`. Hero primary uses `accent`.

### Badge

Mono 11px / uppercase / `0.12em`. Border `ink`. Tones: `neutral` (card/ink), `red` (red/white), `green` (green/white).

### Eyebrow

Mono 12px / uppercase / `0.14em` / `mute`, with an 8px red square prefix.

### Card / Icon tile

Card: `border-ink`, `bg-card`, `p-6`. Elevated adds `shadow-press-lg`.

Prefer one ruled block (shared outer border, internal `border-ink` rules) over a field of separate cards.

### Keycap

22px tall, mono 11px, `border-ink`, `bg-card`, bottom lip `0 2px 0 0 ink`.

---

## Texture

- Page hero: `dot-grid` utility, 24px, `line` dots, masked by content.
- Final CTA (on `ink`): same grid at 6% paper opacity, 20px.
- Ticker: full-bleed `red` bar, mono uppercase paper text, `✱` separators.

---

## Do / don't

**Do**
- Square corners, 1px ink borders, hard shadows.
- One red italic serif word per headline.
- Mono for anything that feels like a label, receipt, or control.
- Alternate `paper` / `paper-deep` for section rhythm.
- Keep the product demo dark on paper — it is a screen, not a card.

**Don't**
- Gradients, blurs, glass, drop shadows with softness.
- Inter on headlines.
- Radius on buttons or cards.
- Extra accent colors on chrome (no blue/purple/pink pills).
- Centered hero with a glow blob.
- Naming competitors on the page.
