# Reel design system

Reel is a precision instrument that happens to be beautiful. The surface is
monochrome, dense, and quiet, with a single accent used as a signal. Nothing is
decorated for its own sake: hierarchy comes from spacing, type, hairline
structure, and short motion.

This is the shared reference for Reel's interface. It describes the design
system that Reel ships today, and `Panel.qml` and `Countdown.qml` are the
reference implementation. When the code and this guide drift, update both in
the same change.

The system is built on Omarchy's tokens in `qs.Commons.Style` and
`qs.Commons.Color`. Reel owns no private palette, so one `omarchy theme set`
should restyle the entire plugin.

---

## 1. How to use this document

### Source of truth

| Concern                           | File                                              |
| :-------------------------------- | :------------------------------------------------ |
| Panel, bar widget, all components | `Panel.qml`                                       |
| Countdown overlay                 | `Countdown.qml` (instantiated inside `Panel.qml`) |
| Runtime state and history shaping | `Service.qml`, `Model.js`                         |
| Plugin registration               | `manifest.json`                                   |
| Deployment and keybinds           | `install.sh`                                      |

---

## 2. Principles

1. **Theme is the brand.** Color comes from the active Omarchy theme. Reel
   contributes layout, rhythm, and restraint, never hues.
2. **Accent is a signal.** Accent marks live state, the primary action, the
   active tab, or focus. It may appear in several roles at once, but never
   twice for the same role, and never as decoration.
3. **Fixed frame, moving content.** The identity, status, history control, and
   primary actions never scroll. Only the page body scrolls.
4. **Density is respect.** A recorder is a tool. Show more, scroll less, and
   use `Style.space()` for every gap.
5. **Motion explains.** Transitions are short, one-shot, and quiet. Motion
   never delays an action.
6. **Keyboard first, one cursor.** Every surface is reachable and operable
   without a pointer. Hover and keyboard share one visual cursor.
7. **Flat, not boxy.** Fill and hairlines create depth. Shadows belong only to
   floating layers, never to rows or buttons.
8. **No layout shift.** Dynamic values get reserved space. State changes never
   move the title, the actions, the tabs, or the scroll origin.
9. **Destruction is explicit.** A destructive action stays neutral until it
   arms, then relabels itself and warns. Urgent color is the warning, not the
   default.
10. **Remove before adding.** If a new feature needs a new surface, delete an
    existing one first or prove it belongs in the current hierarchy.

---

## 3. Tokens & Color

Use these. Do not introduce numeric literals for color, radius, spacing, or type
in layout code.

### Core tokens

| Purpose                       | Token                                                                                |
| :---------------------------- | :----------------------------------------------------------------------------------- |
| Spacing, padding, gaps, sizes | `Style.space(n)`                                                                     |
| Fractional spacing            | `Style.spaceReal(n)`                                                                 |
| Corner radius                 | `Style.cornerRadius`                                                                 |
| Type scale                    | `Style.font.caption` … `Style.font.display`                                          |
| Icon size                     | `Style.font.icon`, `Style.font.iconLarge`                                            |
| Bar geometry                  | `Style.bar.iconSlot`, `Style.bar.iconCanvas`, `Style.bar.iconFont`                   |
| Interactive fills             | `Style.hoverFillFor(...)`, `Style.selectedFillFor(...)`, `Style.pressedFillFor(...)` |
| Borders                       | `Style.normalBorderFor(...)`, `Style.hoverBorderFor(...)`                            |
| Alpha helper                  | `Util.alpha(color, alpha)`                                                           |
| Motion duration               | `Style.duration(ms)`                                                                 |

Panel surfaces read text color from the bar when hosted there:

```qml
readonly property color foreground: root.bar ? root.bar.foreground : Commons.Color.foreground
```

Raw `1` is allowed for hairlines and optical geometry. Custom marks such as the
bar glyph may use unit math (`width / 24`) because they are drawings, not
layout.

### Color roles

| Role       | Alias        | Value                                        | Use                                                   |
| :--------- | :----------- | :------------------------------------------- | :---------------------------------------------------- |
| Foreground | `foreground` | bar foreground or `Commons.Color.foreground` | Text, icons, active controls                          |
| Muted      | `dim`        | `Commons.Color.muted`                        | Meta, hints, inactive labels                          |
| Secondary  | `secondary`  | foreground at `0.72`                         | Readable body text at rest                            |
| Accent     | `accent`     | `Commons.Color.accent`                       | Live state, primary record action, active tab, cursor |
| Urgent     | `urgent`     | `Commons.Color.urgent`                       | Destructive confirmation, errors, live bar clock      |

### State aliases

These live on the panel root and are used instead of local expressions.

| Alias          | Value                | Use                         |
| :------------- | :------------------- | :-------------------------- |
| `restFill`     | foreground at `0.04` | Row and control rest fill   |
| `hoverFill`    | foreground at `0.08` | Pointer hover               |
| `selectedFill` | foreground at `0.14` | Current / pressed-strong    |
| `hairline`     | foreground at `0.12` | Normal border and separator |
| `strongBorder` | foreground at `0.24` | Emphasized border           |
| `accentSoft`   | accent at `0.16`     | Accent-tinted confirmation  |
| `urgentSoft`   | urgent at `0.16`     | Destructive confirmation    |
| `onAccent`     | `inkOn(accent)`      | Ink over accent fills       |
| `onUrgent`     | `inkOn(urgent)`      | Ink over urgent fills       |

`inkOn()` chooses theme background or foreground by contrast ratio. Use it for
any text, glyph, or thumb placed on an accent/urgent fill. Never assume white.

### Alpha ladder

| Surface                 | Alpha  |
| :---------------------- | :----- |
| Rest fill               | `0.04` |
| Hover / keyboard cursor | `0.08` |
| Selected                | `0.14` |
| Pressed                 | `0.22` |
| Hairline                | `0.12` |
| Strong border           | `0.24` |
| Body at rest            | `0.72` |
| Disabled ink            | `0.35` |

Rules:

- No pure black or pure white for content. Black is allowed only for depth:
  keycap shadow, floating-layer shadow, or overlay scrim, never for text, fills,
  or borders on content.
- Never use accent for static headings, ordinary dividers, or decorative icons.
- Never use urgent unless the surface destroys, stops, fails, or warns.
- Do not tint the panel background. It is the theme's popup surface.
- Floating layers use `Commons.Color.background` with accent/urgent borders.
- Light and dark themes must both pass. Test by switching theme, not by
  inverting values.

---

## 4. Typography & Copy

### Typography

The theme font is monospace by default, which suits clocks, values, and keys.

| Token       | Size | Use                                           |
| :---------- | :--- | :-------------------------------------------- |
| `caption`   | 10   | Meta, timestamps, hints, keycaps              |
| `bodySmall` | 11   | Section labels, tab labels, values, shortcuts |
| `body`      | 12   | Default text, rows, buttons, menu items       |
| `subtitle`  | 13   | Reserved for emphasized rows                  |
| `title`     | 14   | Panel title, tooltip icon size                |
| `heading`   | 16   | Empty-state headline                          |
| `display`   | 24   | Hero mark and countdown base size             |

Rules:

- Body text is regular weight. Bold is for values, clocks, active tabs, the
  primary action, and the active state, never for paragraphs.
- Section labels use `caption` + `dim` + bold. Do not uppercase labels.
- The countdown number is `display × 2.2`. It is the only oversized text.
- Numbers that change must not shift width. Use monospace and fixed slots for
  clocks and counters.
- Truncate with `Text.ElideRight`. Use `Text.ElideMiddle` only for paths and
  filenames where both ends matter.
- No text below `caption`. No text above the countdown number inside the panel.
- Wrap text only in prose blocks such as setting help and toast bodies.

### Copy

- Sentence case. “Record”, “Stop”, “Discard?”, “Open the recordings folder”.
- Labels are nouns or short verbs. Values are nouns: `30 fps`, `2.0x`, `Off`.
- Shortcut hints are lowercase key names: `r`, `shift+h`, `enter`. Exception:
  the display form `Shift+H` in tooltips.
- No uppercase UI copy.
- Errors state cause + fix, one line. “No microphone found. Pick one in Audio.”
- No exclamation marks. No “Please”. No marketing voice inside the tool.
- Toasts are one sentence and never modal.
- The countdown label is exactly `Recording starts`.
- Empty state is exactly `No takes yet` + `Record something and it will appear
here.`
- Do not name technical internals in the UI (no `warmup`, `trim-start`, `stage`).

---

## 5. Layout, Spacing & Surfaces

### Spacing scale

`Style.space(n)` is the only spacing unit. The panel is dense by design.

| Step     | Value | Use                              |
| :------- | :---- | :------------------------------- |
| Hairline | 1     | Separators, borders              |
| XXS      | 2     | Icon-to-label, cursor edge inset |
| XS       | 4     | Tight stacks                     |
| SM       | 6     | Row inner padding, glyph gaps    |
| MD       | 8     | Control gap, action gap          |
| LG       | 10    | Panel content gap, toast inset   |
| XL       | 12    | Section gap                      |
| XXL      | 14    | Hero icon-to-title gap           |
| XXL+     | 16    | Tab underline inset              |
| XXXL     | 18    | Empty-state and large gaps       |

### Panel geometry

- Content: **440 × 410** logical pixels (`fittedContentWidth(Style.space(440))`,
  `Style.space(410)`).
- Fixed header: hero (24 high) + `10` gap + actions (38 high) + `10` gap +
  separator (1 high).
- The scroll region receives the remaining height and never changes size.
- The scrollbar is always hidden. Wheel, trackpad, keyboard cursor, and
  edge-hover scrolling are the only scroll inputs.
- Page body spacing: `10`. History list spacing: `8`.
- Row outer padding: `6` left, `6`–`8` right.
- Never add a horizontal margin to the fixed header that the scroll body does
  not share.

### Fixed frame rule

`Panel.qml` uses this structure and must keep it:

```qml
ColumnLayout {
  anchors.fill: parent
  spacing: Style.space(10)

  PanelHero { Layout.fillWidth: true /* status pill + history */ }

  Row { Layout.fillWidth: true /* Record / Pause / Discard */ }

  PanelSeparator { Layout.fillWidth: true }

  ScrollView {
    Layout.fillWidth: true
    Layout.fillHeight: true
    // tabs, settings, history
  }
}
```

Do not put the hero or primary actions back inside the `ScrollView`. Do not
give the `ScrollView` `anchors.fill: parent` inside the layout. Do not set
`implicitHeight` on the key catcher. The header is the frame.

### Layout rules

- Align labels on one left edge. Values are right-aligned and never push a
  label into wrapping.
- Settings row height: `30`. Action height: `38`. Tab height: `30`.
- Take row height: `56`. Thumbnail: `84 × 48`.
- Value chip height: `20`.
- Toggle: `32 × 16`, thumb `12`.
- Icon buttons: at least `22 × 22`, whole and odd where possible.
- Separators are `1` px at `0.12` foreground, used between groups only.
- Media and thumbnails sit on a 2 px grid.
- Reserve space for every dynamic value. The header clock has a fixed `60 × 22`
  slot; the bar width expands from `Style.bar.iconSlot` when recording.

### Surfaces and corners

Default radius is `Style.cornerRadius` for rows, buttons, inputs, cards, menus,
and floating layers. Reel is not a pill-heavy interface.

| Surface                                  | Radius                           |
| :--------------------------------------- | :------------------------------- |
| Rows, buttons, input, cards, menu, toast | `Style.cornerRadius`             |
| Key hint chip                            | `Style.space(4)`                 |
| Value chip                               | `Style.space(4)`                 |
| Toggle, status pill                      | `height / 2` (allowed exception) |
| Countdown                                | circle (`width / 2`)             |
| Glyph tile, thumbnail, shortcut row      | `0` (square)                     |
| Cursor accent edge                       | `width / 2`                      |

Rules:

- Square corners are correct for glyph tiles, thumbnails, and shortcut rows.
- Pills are only for toggles and the status pill. Do not use them for tabs,
  buttons, settings rows, or history rows.
- Do not add radius to a surface that sits inside a row with its own radius.
- Borders are hairlines. Use `hairline` at rest, `accent` for the shared cursor,
  and `strongBorder` only when a surface needs to separate from media.
- Shadows are allowed only on floating layers (toast, take menu, countdown) and
  keycap chips, and only as soft black shadows. Never on rows, tabs, or buttons.
- The primary action is the only permitted gradient in the panel: a two-stop
  vertical accent shade plus a `0.16` top highlight. Do not add gradients
  anywhere else.

---

## 6. Panel anatomy

### 6.1 Bar widget

The bar answers “what is happening?” with a mark and, while recording, a clock.

| State            | Bar                                                   |
| :--------------- | :---------------------------------------------------- |
| Idle             | Drawn screen mark in `Style.bar.iconCanvas`, no label |
| Selecting target | Target glyph `󰒉`, no label                            |
| Countdown        | Countdown number as the glyph                         |
| Recording        | Dot + clock label; clock uses `urgent`                |
| Paused           | Pause glyph, `dim`, no clock                          |
| Saving           | Save glyph, `dim`, no label                           |
| Disabled / error | Never render logs or error text in the bar            |

Rules:

- The mark is drawn, not a Nerd Font glyph: screen outline, center dot, stand.
- Recording width is the measured clock width + `Style.space(30)`.
- The dot pulses only while recording or capturing. Pause stops the pulse.
- Hover uses one shared bar hover fill. No per-state hover color.
- Click opens the panel. Right-click records or pauses. Middle-click opens the
  recordings folder. Tooltips spell this out.

### 6.2 Hero (fixed)

The hero is identity, live status, and history access. It is not a place for
settings, target labels, or metadata.

- Height: content-driven, normally `24`; never taller than one line.
- Mark: drawn screen glyph at `Style.font.display` (24).
- Title: `Reel` or `Reel history`, `title` size, bold, foreground.
- Gap between mark and title: `Style.space(14)`.
- Trailing control contains, in order:
  1. **Recording status pill** — `60 × 22` slot, pill radius.
     - Recording: accent dot + clock, foreground.
     - Paused: secondary dot + secondary clock, no pulse.
     - Idle: opacity `0`; the slot stays reserved.
     - Dot pulse: `0.35 ↔ 1.0`, `900` ms each way.
     - Pill fade: `120` ms.
  2. **History button** — `PanelActionButton`, glyph `󰋚`, accent when the
     history page is active, tooltip `History (Shift+H)` / `Back from history`.
- Do not add a meta line under the title. It changes hero height and moves the
  title off the icon's optical center.
- Do not let the clock label change the hero width. Reserve the slot.

### 6.3 Primary actions (fixed)

Exactly three equal buttons, one row, gap `Style.space(8)`, height `38`.

| Slot | Label                        | Glyph                  | Tone                              | Key |
| :--- | :--------------------------- | :--------------------- | :-------------------------------- | :-- |
| 1    | `Record` / `Stop` / `Cancel` | record / stop / cancel | accent at rest, urgent while live | `r` |
| 2    | `Pause` / `Resume`           | pause / play           | neutral                           | `p` |
| 3    | `Discard` / `Discard?`       | trash                  | neutral, relabels when armed      | `d` |

Rules:

- Primary is the only solid accent surface. It has a subtle vertical shade and a
  top highlight. When a take is live, it switches to urgent so “Stop” reads as
  the interrupt.
- Secondary buttons use `restFill` + `hairline`; cursor uses `hoverFill` +
  accent border.
- Disabled buttons sit at opacity `0.35`, keep their slot, and never move.
- Pressed state scales to `0.985` over `90` ms.
- Discard is a two-step action: first press arms and relabels, second press
  discards. The warning is the relabel, plus a toast if needed.
- Buttons are hidden as a row on the history page, but the hero stays fixed.

### 6.4 Separator

- `1` px, `0.12` foreground, full content width.
- Visible on the home page between the fixed actions and the scrolling body.
- Hidden on history, where the section header provides the break.

### 6.5 Settings tabs

The tab strip is the first thing inside the scroll body. It scrolls with the
settings, not with the fixed frame.

- Height `30`; five equal tabs: Quick, Video, Zoom, Output, Keys.
- Label `bodySmall`; active `accent` + bold; inactive `dim`; hover `foreground`.
- Active underline: `2` px accent, inset `Style.space(16)` from each side.
- Hover underline: `1` px at `0.35` foreground, no fill.
- Every tab carries a numeric `KeyHintChip` (`1`–`5`).
- No pills, no icons, no counters, no badges.
- Switching is instant. Only the underline and label colors animate.

### 6.5.1 Settings taxonomy

The five tabs are fixed. New settings must fit one of them.

| Tab    | Belongs here                                                              | Does not belong                  |
| :----- | :------------------------------------------------------------------------ | :------------------------------- |
| Quick  | Desktop audio, microphone, webcam corner, pointer, manual zoom, countdown | Encoder or file-format details   |
| Video  | Frame rate, quality, codec, resolution cap, container                     | Capture input or output location |
| Zoom   | Zoom depth, pan step                                                      | Anything unrelated to zoom       |
| Output | Saved notification, filename prefix, recordings path                      | Recording inputs                 |
| Keys   | Read-only shortcut reference                                              | Editable key bindings            |

Rules:

- No sixth tab. The `1`–`5` shortcuts are part of the interface contract.
- If a setting fits nowhere, challenge the setting before adding a surface.
- Put the most frequently changed setting first inside a tab.
- Keep the Quick tab to six rows. It is a workflow, not a dump.

### 6.6 Settings rows

- Height `30`, radius `Style.cornerRadius`.
- Structure: glyph tile, label, value.
- Glyph tile: `24 × 24`, square, `restFill` fill, `hairline` border, glyph at
  `Style.font.body` foreground.
- Label: `body`, foreground, elided from the right.
- Value chip (cycle rows): height `20`, radius `4`, `restFill`, `hairline`;
  value at `bodySmall` bold, then `›`. Cursor: `0.10` fill + accent border.
- Info rows: value `bodySmall` bold, secondary, no chip. They still participate
  in the row cursor; activation is a no-op.
- Toggle rows: `32 × 16` pill. On: accent fill, thumb `onAccent`. Off:
  foreground at `0.18`, thumb secondary. Thumb moves in `120` ms.
- Shortcut rows (Keys tab): height `30`, square, key chip + description.
  Key chip minimum width `56`, height `18`, radius `4`, keycap shadow, caption
  bold. Description `bodySmall` secondary.
- The cursor accent edge is `2` px, inset `2` from the left, height row minus
  `10`.
- Clicking a cycle row moves forward. Right-click moves backward.
- Long-press is not used.

### 6.7 Filename prefix input

- Height `30`, radius `Style.cornerRadius`, `restFill`, `hairline`.
- Focus border becomes accent in `120` ms.
- Text `body`, foreground; left/right inset `9`; selection uses accent with
  `onAccent` text.
- Helper line below: `caption`, `dim`, wrapped, explains the default filename.
- This is the only text field in the panel.

### 6.8 History page

**Section header**

- `PanelSectionHeader` at `caption` bold, `dim`.
- Left: `1 take` / `N takes`.
- Right: hint `enter opens · t cuts · a actions · x trashes`, `caption`
  secondary.
- The header scrolls with the list; it is not part of the fixed frame.

**Open folder row**

- Height `30`, radius `Style.cornerRadius`.
- Folder glyph tile `24 × 24`, square.
- Label `body`: `Open the recordings folder`.
- Same cursor and accent edge as settings rows.

**Take row**

- Height `56`, radius `Style.cornerRadius`, clipped.
- Thumbnail `84 × 48`, square, `PreserveAspectCrop`, async.
- Fallback thumbnail: `0.08` foreground fill + play glyph at `title` size,
  `dim`.
- Title: `body`, foreground, bold only on cursor, `ElideMiddle`.
- Meta: `caption`, `dim` (`progress · size · relative time`). Armed trash uses
  `urgent`.
- Overflow button: `PanelActionButton`, glyph `󰇘`, `dim` at rest, accent when
  its menu is open. It is always present but quiet.
- Cursor accent edge: `2` px, inset `2`, height row minus `12`.
- Trash animation: fade `120` ms, then collapse height `160` ms. No spinner, no
  confirmation dialog.
- Click opens the video. Right-click opens the actions menu.

### 6.9 Empty state

- Shown only on the history page with no visible takes.
- Centered in the scroll viewport below the fixed hero.
- Icon: `46` circle with `0.45` accent border and `10` accent dot.
- Headline: `No takes yet`, `heading`, bold, foreground.
- Body: `Record something and it will appear here.`, `bodySmall`, secondary,
  centered, wrapped.
- No action button. The open-folder row is not shown in this state.
- No illustration, no emoji, no mascot.

### 6.10 Toast (notice)

The toast is the only notification surface. It floats over the body.

- Position: bottom of the panel, `10` inset on left, right, and bottom.
- Width: `min(panel width, Style.space(450))`; height: text + `22`.
- Card: `Commons.Color.background`, radius `Style.cornerRadius`, border accent
  at `0.38` (info) or urgent at `0.55` (error).
- Leading bar: `3` px accent or urgent, full height, animates width in `180` ms.
- Medallion: `22` circle, `0.45` accent/urgent ring, `6` dot.
- Text: `body`, foreground or urgent, wrapped, left inset `46`, right inset `14`.
- Progress: `2` px bottom bar, `0.55` opacity, linear, matches the `1900` ms
  lifetime.
- Show: opacity `160` ms, y `200` ms, scale `200` ms with a gentle `OutBack`
  (`1.06` scale overshoot, `1.12` y overshoot). This is the only overshoot in
  the product.
- Hide: opacity `120` ms, y/scale `150` ms, `InCubic`.
- Only one toast at a time. A new notice replaces the old one.
- Saving is a toast, never an optimistic history row.
- Toast is above content (`z: 900`) and below the take menu (`z: 1000`).

### 6.11 Take actions menu

- Width `220`; row height `30`; padding `Style.space(5)`.
- Card: theme background, hairline border, radius `Style.cornerRadius`.
- Depth: two black offset layers, `0.28` and `0.18`, no blur.
- Row: glyph `20` wide, label `body`, shortcut `caption` secondary.
- Items: `Open` (`enter`), `Cut in Omacut` (`t`), `Share` (`u`), `Delete`
  (`x`). Delete arms first, then relabels to `Confirm delete` in urgent ink.
- `c` copies the selected take's path without opening the menu.
- Cursor row: `selectedFill` + `2` px accent leading edge; destructive rows use
  `urgentSoft` + urgent ink.
- Menu appears and dismisses instantly. No slide, fade, or bounce.
- Dismiss on outside click, `esc`, or after an action.

### 6.12 Countdown

The countdown is a visual, non-blocking overlay hosted **inside `Panel.qml`**.
It must never become a separate overlay plugin kind.

- Overlay window: `reel-countdown`, no keyboard focus, empty input mask.
- Ring: `200 × 200` circle, theme background at `0.94`, border `0.16`
  foreground at `2` px, accent progress arc `3` px with round caps.
- Depth: two black circular shadows at `0.24` and `0.18`.
- Number: accent, `display × 2.2`, bold, pulse from `1.12 → 1` in `190` ms.
- Label: `Recording starts`, `bodySmall`, foreground at `0.68`.
- Ring progress animates in `240` ms `OutCubic`.
- Countdown default is **Off**. Options are `0`, `3`, `5`, `10` seconds.
- The UI must not imply that capture starts after the ring.

### 6.13 Tooltips

- Use `PanelToolTip` from `qs.Ui`. Never build a custom tooltip.
- Text includes the shortcut: `History (Shift+H)`.
- Hover only. No tooltip on keyboard focus.
- One line, `caption`, sentence case.

---

## 7. Interaction model

### One cursor

The panel root owns all cursor state:

- `cursorActive`
- `focusSection`: `header`, `primary`, `settings`, `recents`
- `selectedIndex` / `settingsIndex`
- `hoverSection` / `hoverIndex`

Every interactive row wraps its content in `CursorSurface` or consumes
`hasCursor` from the root. A component **must not** read `containsMouse` for its
own color or border. Hover updates the same root state as the keyboard, which
guarantees exactly one highlight on screen.

The shared cursor paint is:

- fill `Style.hoverFillFor(foreground, accent)`
- border `Border.controlSpec("hover-cursor", foreground, accent)`
- leading accent edge `2` px on list rows

### States

| State                   | Treatment                                                |
| :---------------------- | :------------------------------------------------------- |
| Rest                    | `restFill` for buttons/fields; transparent for list rows |
| Hover                   | `hoverFill`                                              |
| Keyboard cursor         | Same as hover, plus a `1` px accent border               |
| Current / selected      | `selectedFill`, accent leading edge                      |
| Pressed                 | `selectedFill` (buttons also scale `0.985`)              |
| Disabled                | opacity `0.35`, no hover, no cursor change               |
| Focus ring (text input) | accent border, `120` ms                                  |

Rules:

- Keyboard cursor and hover must be visually identical except for the accent
  border.
- Focus is always visible. Never remove a border without replacing it.
- Hit targets are at least `22 × 22`; row targets are `30` or `38` high.
- Pointer cursors: pointing hand for actions, arrow for info rows and text.

### Keyboard

| Key               | Action                               |
| :---------------- | :----------------------------------- |
| `↑` `↓` / `j` `k` | Move between rows                    |
| `←` `→`           | Move across the action row           |
| `h` `l`           | Move horizontally or cycle an option |
| `enter` / `space` | Start, stop, save, open, or activate |
| `tab`             | Switch sections or settings tabs     |
| `1`–`5`           | Jump to a settings tab               |
| `r`               | Start or stop a take                 |
| `p`               | Pause or resume                      |
| `d`               | Discard the current take             |
| `shift+h`         | Open or close history                |
| `s`               | Focus the settings tabs              |
| `t`               | Cut the selected take in Omacut      |
| `a`               | Open the take actions menu           |
| `x` / `delete`    | Trash the selected take              |
| `u`               | Share the selected take              |
| `c`               | Copy the selected take's path        |
| `o`               | Open the recordings folder           |
| `q`               | Close the panel                      |
| `esc`             | Go back, then close                  |

Rules:

- `esc` cancels a selection or countdown, but never stops an active take.
- The panel may close while recording without stopping the take.
- Movement triggers `ensureCursorVisible()` so the cursor never hides behind
  the fixed header or the panel edge.
- Typing in the filename prefix blocks single-key shortcuts.

### Scrolling

- Hidden scrollbar. Wheel and trackpad work normally.
- Hovering within `64` px of the top or bottom edge starts silent auto-scroll
  while the pointer stays in that zone. There is **no visible edge indicator
  line**.
- Auto-scroll runs on a `16` ms timer and accelerates from `3` to `7` px per
  tick. It stops at the content bounds.
- The fixed header is not part of the scroll region.
- Opening the panel and entering history reset the scroll to the top.

---

## 8. Motion

| Moment                                  | Duration           | Easing             |
| :-------------------------------------- | :----------------- | :----------------- |
| Hover / cursor color (shell primitives) | `60` ms            | OutCubic           |
| Button press scale                      | `90` ms            | OutCubic           |
| Hover, focus, border, chip              | `120` ms           | OutCubic           |
| Tab underline/color, trash fade         | `120` ms           | OutCubic           |
| Toast hide                              | `120`–`150` ms     | InCubic            |
| Trash row collapse                      | `160` ms           | OutCubic           |
| Toast show                              | `160`–`200` ms     | OutCubic / OutBack |
| Leading toast bar                       | `180` ms           | OutCubic           |
| Countdown pulse                         | `190` ms           | OutCubic           |
| Countdown ring                          | `240` ms           | OutCubic           |
| Status pill pulse                       | `900` ms each way  | InOutQuad          |
| Bar recording dot                       | `1000` ms each way | InOutQuad          |

Rules:

- Animate opacity, color, small offsets, and small scale only.
- Never animate width, height, or font size — except the explicit trash row
  collapse, which exists to remove a row without a jump.
- No spring, no generic bounce. The toast is the only overshoot, and it is
  capped at `1.06` scale / `1.12` y.
- Wrap every duration in `Style.duration(ms)`.
- The recording dot and status pill dot are the only looping animations.
- Respect `Style.reduceMotion`. Infinite loops must be disabled, not shortened.
- No animation on scroll.
- No spinner. Progress is shown by a ring, a bar, or a state label.

### Signature moments

Spend polish only here:

1. **Start.** The bar mark becomes a dot and the clock widens into place.
2. **Open.** The panel fades and lifts once; nothing inside jumps.
3. **Save.** The toast confirms the save and the new take appears at the top of
   history.

If a fourth signature animation is proposed, remove one of these instead.

---

## 9. Iconography

- Nerd Font glyphs only. No emoji, no mixed icon sets.
- The bar mark and hero mark are drawn geometry, not font glyphs.
- One glyph per row, optical size `12`–`14`, `muted` or `secondary` at rest.
- Glyph tiles are `24 × 24`, square, `restFill`, `hairline`.
- A glyph never replaces a text label for a primary action.
- Icon-only buttons are allowed for history, overflow, and tooltip-backed
  actions. Every one needs a tooltip and a keyboard path.
- Optical centering beats mathematical centering. Nudge inside the slot when a
  glyph looks low.

---

## 10. Accessibility

- Body and labels must remain readable at `caption` and above. `muted` is for
  meta only, never for a label the user must read to act.
- State is never carried by color alone: pair accent with a dot, weight, glyph,
  label, or position.
- Everything reachable by pointer is reachable by keyboard, in the same order.
- Respect `Style.reduceMotion`, `Style.font` scale, and spacing scale. No fixed
  pixel font sizes in UI text.
- Use `inkOn()` for text on accent/urgent fills.
- Test at `base-size` 10, 12, and 16, in a light theme, and with reduced motion.
- The countdown never takes focus and never blocks the desktop.
- Trash is reversible (system trash) and optimistic. Save is confirmed by toast.
- Keep the panel usable while recording. Closing the panel never stops a take.

---

## 11. Engineering contract

### Do

- Use `Style.*`, `Commons.Color.*`, and `Util.alpha()`.
- Use `CursorSurface`, `PanelActionButton`, `PanelHero`, `PanelSectionHeader`,
  `BorderSurface`, `KeyboardPanel`, and `PanelToolTip` from `qs.Ui`.
- Keep one source of cursor state on the panel root.
- Keep the fixed header structure from §5.
- Keep `contentHeight: Style.space(410)` and
  `fittedContentWidth(Style.space(440))`.
- Add a `Behavior` for every color/border/opacity change that has a state.
- Update this document in the same change as any visual change.

### Do not

- Do not introduce a private palette or hardcoded hex for UI color.
- Do not add a second hover style, focus style, or highlight.
- Do not put dynamic content in the fixed header without a reserved slot.
- Do not add a live preview, slow motion, or a second overlay window.
- Do not add spinners, skeletons, shimmer, or loading illustrations.
- Do not add nested modals. Use the take menu or a toast.
- Do not add a visible scroll-edge indicator.
- Do not restyle Omarchy primitives locally. Fix the shared component instead.
- Do not use emoji or marketing copy.
