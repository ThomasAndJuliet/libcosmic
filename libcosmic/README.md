# libcosmic highlight standard (household-ui rule 6): before and after

Change: branch `fork/highlight-standard` of ThomasAndJuliet/libcosmic, HEAD ddb155bd (6 commits on f7ed6c4b). Text boxes and default text editors draw selected text on a solid accent fill, recoloured by the fill rule; the fill is drawn in the redrawn text's own layer, so no fringe of the unselected text colour shows around selected glyphs.

All frames: debug builds in the agent-vnc container on neo (cosmic-comp, software rendering), 1920x1080, scale 1, lossless PNG, nothing scaled. Light and dark. Diffs: `ct-fill-greys/scripts/ui-compare`, tolerance 0.

## Juliet's words these answer

1. "solid for things like the composer in discord/terminal": text boxes and the composer show a solid accent fill, before and after (the fill itself did not change; it was already solid).
2. "there is not real reason to prefer grey over white": the selected text in the frames is white (light) or black (dark) on the fill, not grey.
3. Rule 6, no fringe of the unselected colour around selected glyphs: the only pixels that changed are glyph edge pixels (see the findings).

## Builds (labels on neo, builds kept)

| What | Worktree | Label | libcosmic |
|---|---|---|---|
| text-input before | `~/Code/libcosmic-hl-before` (detached f7ed6c4b) | `hl-ti-before` | f7ed6c4b |
| text-input after | `~/Code/libcosmic-highlights` | `hl-ti-after` | ddb155bd |
| COSMIC Terminal before | `~/Code/ct-fill-greys` (ec49c157), the existing build copied from `hl-after` | `hl-lc-before` | 8f49ab31 |
| COSMIC Terminal after | `~/Code/ct-hl-fork` (ec49c157 plus the patch change, uncommitted) | `hl-lc-after` | ddb155bd |

## Files

`before/` and `after/` hold whole-window frames; `<app>-<theme>-<NN>-<case>.png`, same name in both.

| Stem | Shows |
|---|---|
| `text-input-<theme>-00-start` | the example as it opens (two fields, "Test") |
| `text-input-<theme>-01-editable-unselected` | the editable field after the pencil is clicked and " one two" typed; nothing selected |
| `text-input-<theme>-02-editable-selected` | the same field, "one two" selected with Ctrl+Shift+Left twice |
| `text-input-<theme>-03-inline-selected` | the inline field below, "one two" selected the same way |
| `terminal-<theme>-10-conversation-start` | COSMIC Terminal, scene `gaussian`, orange Claude tab in front, composer empty |
| `terminal-<theme>-11-composer-unselected` | the composer with "The numeric check agrees to ten digits, so the Gaussian integral is sqrt(pi)." typed |
| `terminal-<theme>-12-composer-selected` | the same, "is sqrt(pi)." selected (Ctrl+Shift+Left three times) |
| `terminal-<theme>-20-settings-unselected` | File > Edit profiles..., the Claude profile opened, "Gaussian integral check" typed into Tab title |
| `terminal-<theme>-21-settings-selected` | the same, "integral check" selected (Ctrl+Shift+Left twice) |

`crops/`: 1:1 crops of each selection, `<stem>-before.png`, `<stem>-after.png`, `<stem>-pair.png` (before above after, a magenta line between). Selected cases only plus the text-input unselected one: 360x110 at (780,40) for text-input; 1000x70 at (455,912) for the composer; 360x60 at (1296,570) for the Settings field.

`diff/<component>/<scene>.png`: changed pixels magenta over a faded "after", regions boxed and numbered. Components: `text-input`, `composer` (terminal frames 10 to 12), `settings` (20 and 21). `diff/summary-text-input.md` and `diff/summary-terminal.md` list every region.

`logs/`: build and check logs (each ends with `exit N`), and the `Cargo.toml` and `Cargo.lock` diffs of the two throwaway worktrees.

## Findings

1. **The visible change is only the glyph edge pixels.** Every selected frame differs from its before in one region, exactly the selected text: 252 px (light) and 257 px (dark) in the text-input example, 469 px in the composer, 455 px in the Settings field. The fill colour is identical before and after (#00525a light, #63d0df dark in the example). In the before frames the antialiased edges of the selected glyphs carry the grey of the unselected text (for example (213,210,208) where the after has (231,215,208), on the orange fill); in the after they are blended with the fill. This is the fringe fix, and at 1:1 it is subtle. It is not a change of the selected text's colour: the example's text was already white (light) and black (dark) on the fill before, and so was the terminal's, because the BEFORE terminal build (ct-fill-greys) already carries the app-side fill rule.
2. **Nothing else changed.** The only other differing pixels are caret blink: a 1 px wide, 20 to 28 px high bar in `text-input-*-01-editable-unselected` (947,70), `terminal-light-10-conversation-start` (480,923) and `terminal-light-20-settings-unselected` (1467,588). The unselected frames are otherwise identical, and the whole-window diffs show no change outside the selections.
3. **No disabled or secondary field.** The text-input example has two fields only (an editable one and an inline one), so no disabled or secondary field was shot. The "editable" field draws without a box until it is edited.
4. **The composer reshoot.** The old `evidence/terminal/after/*07-composer*` was not reused: its exact steps were not recorded, so both builds were shot again here with the steps above.
5. **Not covered:** the opt-in `selection_tint` (no app uses it), and any video (no behaviour changed).
6. In dark with the orange tab in front the window border stays teal (the system accent); it is the same before and after.

## What differs from the brief

- BEFORE for the example is f7ed6c4b (the fork's base), not the current `origin/household` (68bedba4, which has moved on by commits that are not part of this change). `Cargo.lock` is untracked in libcosmic; the before worktree was given a copy of the after worktree's lock, so both build the same dependency versions.
- The fork branch is already on origin (`fork/highlight-standard` = ddb155bd), so the [patch] entries use `branch = "fork/highlight-standard"` on the existing ssh URL (all 29 entries, both the libcosmic and the winit family). neo cannot fetch private ThomasAndJuliet repositories, so ddb155bd was pushed into neo's cargo git database (`~/.cargo/git/db/libcosmic-bd0cc8a7924f73dc`, a new branch `hl-fork-highlight-standard`); the build then found the locked revision there and fetched nothing.

## Look at these first: the change, zoomed
At 1:1 the before and after look alike; the change is the grey fringe of the unselected text colour that showed around selected letters. Nearest-neighbour ×8, before above after:
- crops/zoom-composer-light.png, crops/zoom-composer-dark.png (Terminal composer, a text editor)
- crops/zoom-text-box-light.png, crops/zoom-text-box-dark.png (libcosmic's text-input example)
- crops/zoom-settings-field-light.png, crops/zoom-settings-field-dark.png (a Terminal Settings field)

## Agent's choices, for Juliet's ruling
- "The on colour" is the theme's own (COSMIC's accent.on): white on the light accent, black on the dark theme's pale accent. The sheet showed the light theme only and said "white".
- The text-input example shows the system accent (teal), not an orange tab: it has no tabs.
- Editors leave syntax-highlighted spans as they are (stage 2), so "colours keep their hue" is not shown by any frame here; plain text is.
- The Terminal "before" already had its own grey-to-white rule, so these frames show only the fringe fix, not the #ababab grey that started this (that is in ../terminal/).
- No video: nothing behaves differently; selection works as before.
