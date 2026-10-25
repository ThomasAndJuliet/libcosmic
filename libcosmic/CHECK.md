# Independent check: libcosmic highlight standard evidence

Checker: a separate agent that did not make the change or the captures. Read-only apart from this file and the new `crops/zoom-*.png`.

## Verdict: ready to show, with two small additions to the README (below)

The after frames meet the rule, nothing outside the selections changed except caret blink, and the fringe fix is real and measurable. The visible change is tiny (antialiased edge pixels only), so I made labelled zoomed views (`crops/zoom-*.png`); show those first. The README needs the zoom files linked, and an "agent's choices for your ruling" list (section 4). Nothing in the frames needs re-shooting.

## 1. After frames against the rule

Opened: all 18 `after/` frames, all 10 crop pairs (every selected case plus the text-input unselected one), and diffs `composer/light-12`, `text-input/dark-02` (the other diffs are covered by the numbers below).

Pixel values (selection box from `diff/summary-*.md`, 1 px margin):

| Frame | Fill before = after | Selected text (core) | Edge pixels, before | Edge pixels, after |
|---|---|---|---|---|
| terminal-light-12 (composer) | (126,37,0) #7E2500, 1310 px both | white (255,255,255), 84 px both | 448 px more than 3 RGB units off the fill-to-white line, worst 28.4; e.g. (206,202,200) | 0 px off the line, worst 2.6; the same pixel is (227,208,200) |
| terminal-dark-12 (composer) | (255,162,132) #FFA284, 1310 px both | black (0,0,0), 84 px both | 465 px off the line, worst 26.6; e.g. (49,45,44) | worst 3.6 (23 px, rounding: (55,35,28) is on the line at 0.216 of the fill in all three channels) |
| terminal-light-21 (Settings field) | (126,37,0) | white | 440 px off, worst 24.8 | 0 inside the selection (the 16 px flagged are the field's own background column at x=1468, (212,212,212), identical before and after, outside the fill) |
| terminal-dark-21 | (255,162,132) | black | 444 px off, worst 28.2 | same, 16 px are the background column |
| text-input light 02 and 03 | (0,82,90) #00525A | white | 244 px off, worst 21.2 | the 12 flagged are the background column at x=947, (215,215,215), same before |
| text-input dark 02 and 03 | (99,208,223) #63D0DF | black | 250 px off, worst 30.4 | same, 12 px background column |

Reading: in every after frame each pixel inside the selection lies on the straight line between the fill colour and the on colour, which is what a glyph antialiased against the fill looks like. In every before frame about 250 to 465 pixels sit off that line: they carry the grey of the unselected text. So no fringe of the unselected colour remains, and the fill is byte-identical to before (same colour, same pixel count). Selected text is white on the light fills and black on the dark fills, as the brief says.

The frames and crops look right at 1:1: solid accent boxes, readable text, no stray colours, light and dark.

## 2. Anything changed outside the selections, or not asked for

Whole-frame comparison of all 18 pairs (`compare -metric AE`, bounding box of changed pixels):

- Identical (0 px): terminal-light-11, terminal-dark-10, -11, -20, text-input-light-00, text-input-dark-00.
- Changed only in the selection box: terminal-{light,dark}-12 (89x21 at 1078,929), terminal-{light,dark}-21 (90x14 at 1378,591), text-input-{light,dark}-02 and -03 (52x10 at 895,75 and 895,117).
- Changed only by the caret: text-input-{light,dark}-01 (1x20 at 947,70), terminal-light-10 (1x28 at 480,923), terminal-light-20 (1x19 at 1467,588). I checked the pixels: the caret is a dark bar in one frame and absent in the other (e.g. text-input-light-01 at (947,75): before (18,18,18), after (215,215,215); in the dark pair it is the after frame that has it), so this is blink phase, not a change. The code diff in the editor and input touches only selection quads and text colour, not the caret.

Nothing else moved. The README's claim holds.

## 3. Is the change understandable to Juliet

On its own, no: the crops at 1:1 look the same before and after (I could not tell the pairs apart by eye in `crops/*-pair.png`). The README says so honestly, but the evidence did not carry a zoomed view. I made six, nearest-neighbour x8, a 40x20 px region at a glyph edge, before above after, labelled with the build and the region:

- `crops/zoom-composer-light.png`, `crops/zoom-composer-dark.png` (the terminal composer; before = libcosmic 8f49ab31)
- `crops/zoom-text-box-light.png`, `crops/zoom-text-box-dark.png` (the libcosmic text-input example; before = f7ed6c4b)
- `crops/zoom-settings-field-light.png`, `crops/zoom-settings-field-dark.png` (the Settings Tab title field, an extra)

Viewed: in the before panels the edge pixels of "is sq" (composer) are a muted grey-tinted halo; in the after panels they blend cleanly into the fill (a pinker or paler tone of the fill in light, a darker tone of the fill in dark). The difference is visible at x8 and is the whole story. Suggest showing these first and the 1:1 frames after, with one line saying "the glyph edges lose a faint grey halo; nothing else changes".

## 4. Gaps against the skill, and what Juliet should rule on

Met: light and dark; whole-window frames at 1920x1080, scale 1, lossless; pixel diffs for every still; traceability to her quoted words; an independent check (this).

Gaps and notes:

1. **No agent's-choices list.** The README has "What differs from the brief" but not the choices for her ruling. From the commits and frames, these are the ones to list:
   - Selected text goes to the theme's own on colour (COSMIC's `accent.on`) rather than "white" as `sheets/highlight-standard.png` says (the sheet shows light only). In light that is white; in dark it is black on COSMIC's pale accent (#FFA284 here). Her "no real reason to prefer grey over white" and "COSMIC's defaults kept" support it, but she has not been shown the dark case on the sheet. Flag it plainly: dark = black text on a light accent.
   - The text-input example uses the system teal accent, not an orange Claude tab, so it is not what she sees in the terminal; the terminal frames do use the orange tab.
   - The editor leaves the highlighter's coloured spans untouched on the fill ("stage 2" in the code comment). So "colours keep hue" is not yet exercised and no frame shows coloured selected text; the rule's hue-keeping is not demonstrated by this evidence.
2. **The terminal "before" is not the shipped look.** `hl-lc-before` already has the app-side fill rule (greys to white), so these frames show only the fringe fix, not the "dark composer text was #ababab" case that prompted the rule. If she wants to see that change, a before from the household terminal without the app-side patch is needed. Not a defect in the change; it limits what the frames can show.
3. **No video.** None needed for a still colour change; selecting and typing behave as before, but the evidence does not show that. Optional.
4. **README should link the zoom files** and list the agent's choices above. I did not edit the README (read-only for me).
5. The 1:1 crops and the 1:1 diffs do not make the change visible; the diff overlay shows magenta glyph-shaped regions but gives no sense of size. The zooms are the fix.

## 5. Minor

- The label on `diff/summary-*.md` says "before: /tmp/hl/cm after: /tmp/hl/cm" (a scratch path); harmless but confusing.
- Dark terminal frames show a teal window border with the orange tab in front (README item 6): same before and after.
