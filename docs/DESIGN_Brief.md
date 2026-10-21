# LYRA — Premium Hi-Fi Frontend Design Brief

## 1. Product Vision

Lyra is a premium, Linux-native audiophile music player, powered by the UNDA audio engine.

The interface must embody the craftsmanship, restraint, and precision of high-end Hi-Fi equipment.

**Our ambition is to create the software equivalent of a beautifully engineered reference-grade audio component.**

Lyra must feel:

- **Ultra-premium:** sophisticated, refined, and meticulously crafted.
- **Minimalist:** no unnecessary visual elements or decorative noise.
- **Purposeful:** every component, control, and interaction has a reason to exist.
- **Intuitive:** advanced functionality without a complicated interface.
- **Calm:** the music remains the center of attention.
- **Precise:** consistent spacing, alignment, typography, and interaction behavior.
- **Timeless:** avoid fashionable UI effects that will look dated in two years.

Minimalism must never mean removing useful functionality. The challenge is to make a fully featured audiophile application feel effortlessly simple.

**The user should notice the music, not the interface.**

## 2. Design References

Study the design language and interaction patterns of established premium music players and Hi-Fi manufacturers.

### Software references

- **Oxitide:** modern, restrained music-player interface and thoughtful use of space.
- **Roon:** rich music browsing, metadata presentation, and advanced audio information.
- **Audirvāna:** audiophile-oriented playback, signal-path visibility, and device management.
- **Sonos:** intuitive controls, clean navigation, and approachable music management.

### Hardware references

Draw inspiration from premium audio equipment by:

- Naim Audio
- Linn
- Cambridge Audio
- Devialet
- NAD Masters
- dCS
- Aavik

Study how these brands communicate quality through typography, material finishes, symmetry, proportions, tactile controls, and subtle lighting.

**Do not copy their interfaces or visual identities.** Extract the underlying design principles and build a coherent identity for Lyra.

Avoid making Lyra look like Spotify, a generic SaaS dashboard, or an Electron developer tool.

## 3. Visual Identity

### Overall aesthetic

Use a sophisticated, predominantly dark interface.

The design should evoke:

- Anodized black aluminum.
- Graphite and charcoal.
- Brushed metal.
- Precision-machined controls.
- Soft ambient lighting.
- High-resolution displays.
- Premium industrial design.

This does not mean reproducing physical knobs or switches literally.

Use modern, understated digital components with the visual discipline of premium hardware.

### Color palette

Establish a coherent semantic color system using design tokens.

Suggested starting palette:

| Token | Suggested value | Purpose |
|---|---|---|
| Background | `#0D0F12` | Main application background |
| Surface | `#15181C` | Panels and secondary regions |
| Elevated surface | `#1C2025` | Menus and dialogs |
| Primary text | `#F1F2F3` | Primary content |
| Secondary text | `#9A9FA7` | Supporting information |
| Muted text | `#666D76` | Low-priority metadata |
| Border | `#292E35` | Subtle structural separation |
| Accent | `#B8C7D9` | Reserved interaction accent |

These values are initial guidance, not mandatory final values. Refine them as a complete system.

Avoid pure black and pure white everywhere. Use subtle tonal differences to establish depth.

### Accent colors

Lyra's visual identity should remain predominantly monochromatic.

Use accent colors sparingly for meaningful interactions and states.

UNDA's identity may use a spectral color gradient, but it must not dominate the Lyra interface.

Reserve spectral colors for audio-specific visualizations, such as a signal-path display or a deliberately enabled spectrum visualization.

**No gratuitous gradients, neon effects, or excessive glow.**

## 4. Typography

Typography is one of the most important elements of the design.

Use a modern, refined sans-serif typeface with excellent readability and carefully balanced proportions.

Consider fonts such as Inter, Geist, or another similarly restrained family with appropriate licensing.

Establish a consistent typographic scale.

Principles:

- Strong but understated hierarchy.
- Carefully controlled font weights.
- Generous breathing room around important content.
- Consistent line heights.
- Excellent legibility at small sizes.
- Tabular numerals for time, sample rates, bit depths, and technical measurements.
- Avoid excessive uppercase text and letter spacing.
- Avoid oversized headings unless they serve a clear editorial purpose.

Track titles and album artwork deserve prominence.

Technical metadata should be readable but visually secondary.

## 5. Layout and Information Architecture

The application must be organized around the user's listening experience.

### Primary navigation

Keep navigation compact, predictable, and unobtrusive.

Potential sections include:

- Home
- Albums
- Artists
- Tracks
- Playlists
- Queue
- Audio Devices
- Settings

Do not expose every feature in the primary navigation.

Group related functionality logically.

Use progressive disclosure for advanced controls.

### Main content area

Prioritize album artwork, album information, and music browsing.

Use consistent grids, spacing, and alignment.

Avoid excessive card borders and nested panels.

Let artwork provide much of the visual richness.

The interface should remain calm even when displaying large music libraries.

### Persistent playback controls

Playback must remain accessible throughout the application.

Create a refined, persistent player containing:

- Current album artwork.
- Track title and artist.
- Play/pause.
- Previous/next.
- Playback progress.
- Volume control where supported.
- Queue access.
- Output device.
- Audio format/status indicator.

Use a clear hierarchy.

Primary playback controls should be immediately discoverable.

Secondary controls should remain discreet.

The player must never feel like a crowded toolbar.

## 6. Component Design System

Create a small, coherent set of reusable components.

Every component should follow shared design tokens and interaction rules.

### Buttons

- Clear primary, secondary, tertiary, and icon-only variants.
- Minimal borders and backgrounds.
- Subtle hover and pressed states.
- Consistent hit areas.
- Elegant focus indicators.
- No unnecessary shadows.
- No exaggerated animations.

### Sliders

Pay particular attention to the volume and playback progress sliders.

They should feel exceptionally precise.

Requirements:

- Thin, refined tracks.
- Clearly differentiated filled and unfilled portions.
- Subtle handles, visible when appropriate.
- Smooth pointer interaction.
- Keyboard accessibility.
- Accurate numerical values.
- No visual jitter during updates.

### Menus and dropdowns

- Compact and well-aligned.
- Clear grouping.
- Subtle separators.
- Consistent item heights.
- Excellent keyboard navigation.
- No unnecessary nested menus.
- Appropriate positioning near the triggering control.

### Dialogs

Dialogs should feel like carefully designed instrument panels.

Use them only when necessary.

Prefer lightweight popovers or inline editing for simple operations.

### Lists and tables

Music libraries require information-dense interfaces.

Design for efficient browsing without looking like enterprise software.

- Precise column alignment.
- Subtle hover states.
- Clear selection states.
- Optional album artwork thumbnails.
- Consistent row heights.
- Excellent readability.
- Efficient handling of large libraries.
- Minimal visible borders.

### Icons

Use a consistent icon family.

Prefer fine, carefully balanced strokes.

Avoid mixing filled, outlined, and differently weighted icon styles without a semantic reason.

Icon-only controls must have accessible labels and tooltips where useful.

### Loading and empty states

Avoid generic spinning indicators everywhere.

Use subtle skeletons or understated progress feedback where appropriate.

Empty states should explain the next useful action without unnecessary illustration or marketing language.

## 7. Audiophile-Specific Experience

Lyra is not a generic music player.

Its interface must communicate the quality and integrity of the audio signal without overwhelming the user.

### Playback format display

Support clear, discreet presentation of:

- PCM
- DSD
- DoP
- Sample rate
- Bit depth where applicable
- Channel configuration
- Output device

Examples:

`PCM · 24-bit / 192 kHz`

`DSD256 · Native`

`DSD128 · DoP`

These indicators must reflect actual backend-reported playback state, not assumptions derived solely from file metadata.

### Signal path

Design a dedicated signal-path view inspired by professional audiophile software.

The user should be able to understand:

1. Source file and encoding.
2. Decoding or processing performed.
3. Any sample-rate conversion or format conversion.
4. Actual output format.
5. ALSA device and DAC.

Use a clean, technically accurate visual hierarchy.

Do not invent processing stages or claim bit-perfect output unless the engine can verify the relevant conditions.

This view should be accessible on demand, not permanently occupy the main player.

### Device management

Audio output selection should be simple for ordinary users and informative for advanced users.

Present:

- Device name.
- Connection status.
- Supported formats when detected.
- Active output format.
- Relevant hardware capabilities.

Keep advanced ALSA settings behind an appropriately organized settings panel.

### Bit-perfect status

If the backend can establish bit-perfect playback conditions, provide a discreet status indicator.

Do not use misleading claims such as "lossless" or "bit-perfect" merely because a FLAC or DSD file is playing.

## 8. Interaction and Motion

Interactions should feel deliberate and polished.

Use subtle transitions to communicate state changes.

Suggested animation durations:

- Hover and focus: 100–150 ms.
- Small state transitions: 150–200 ms.
- Panel transitions: 180–250 ms.

Use restrained easing curves.

Avoid:

- Bouncy animations.
- Excessive scaling.
- Parallax.
- Constantly animated gradients.
- Decorative motion.
- Long transitions that delay interaction.

Respect the user's reduced-motion preferences.

**The interface should feel responsive before it feels animated.**

## 9. Accessibility and Usability

Premium design must not compromise usability.

Requirements:

- Full keyboard navigation for essential actions.
- Visible keyboard focus.
- Accessible control names.
- Adequate text contrast.
- Predictable navigation.
- Sensible minimum hit targets.
- Tooltips where icon meaning is not immediately obvious.
- Proper semantic HTML.
- Screen-reader support for important playback states.
- Reduced-motion support.
- Responsive behavior across supported desktop window sizes.

Do not make secondary information so faint that it becomes difficult to read.

Discretion and invisibility are not the same thing.

## 10. Technical Implementation

The frontend uses Svelte 5, TypeScript, and Tauri 2.

### Architecture

- Prefer idiomatic Svelte 5 components and reactivity.
- Use TypeScript strictly.
- Keep components small and focused.
- Separate presentation from backend integration.
- Establish clear contracts between the UI and Rust backend.
- Avoid duplicating backend state unnecessarily.
- Do not perform audio processing in the frontend.
- Avoid unnecessary global stores.
- Avoid excessive component abstraction.
- Avoid large UI frameworks unless justified.
- Prefer CSS variables and a small design-token system.
- Use a consistent spacing and sizing scale.

Apply the project's TypeScript anti-slop guidelines.

### Performance

The frontend must remain responsive when:

- Browsing large music libraries.
- Loading album artwork.
- Searching thousands of tracks.
- Updating playback progress.
- Switching audio devices.
- Displaying technical playback information.

Use virtualization where necessary for large lists.

Avoid unnecessary rerenders and expensive reactive computations.

Avoid high-frequency UI updates that do not provide perceptible value.

### Maintainability

Create reusable components when genuine reuse exists.

Do not create abstraction layers for hypothetical future requirements.

Prefer straightforward, readable implementations over clever solutions.

All components must use the shared visual system.

Do not introduce arbitrary one-off spacing, colors, border radii, or animation timings.

## 11. Screens to Design and Refine

Evaluate the existing frontend and progressively establish a consistent experience across:

1. **Now Playing:** the most emotionally engaging screen, with album artwork, track information, and refined playback controls.
2. **Library:** albums, artists, tracks, search, sorting, and filtering.
3. **Album Details:** artwork, metadata, track listing, and playback actions.
4. **Queue:** upcoming tracks and intuitive queue management.
5. **Audio Devices:** device selection and output capabilities.
6. **Settings:** logically grouped general, library, appearance, and audio preferences.
7. **Signal Path:** technically accurate audio-processing information.

Do not create nonfunctional screens merely to make the application appear complete.

If backend functionality is not implemented, distinguish clearly between implemented behavior and planned UI.

## 12. Quality Bar

Before considering a screen complete, evaluate:

- Does every visible control have a purpose?
- Is the hierarchy immediately understandable?
- Is the spacing consistent?
- Are all elements precisely aligned?
- Is the typography balanced?
- Are icons consistent?
- Is the screen visually calm?
- Can a new user find the essential actions?
- Can an experienced audiophile access the advanced functionality?
- Are hover, focus, disabled, loading, and error states designed?
- Does the screen work at smaller window sizes?
- Is the implementation accessible?
- Is the code simple and maintainable?
- Does it feel like a premium Hi-Fi product rather than a generic web application?

If a component looks out of place, reconsider it.

If a control is unnecessary, remove it.

If information is important but visually overwhelming, redesign its presentation rather than hiding it arbitrarily.

## 13. Implementation Approach

First, inspect the current frontend architecture, components, styles, and screens.

Identify inconsistencies and opportunities to simplify the design system.

Then:

1. Define the visual tokens and component foundations.
2. Establish typography, spacing, surfaces, and interaction standards.
3. Refine the application shell and navigation.
4. Perfect the persistent playback controls.
5. Refine the Now Playing experience.
6. Apply the design system consistently to the library and remaining screens.
7. Validate accessibility, responsiveness, and visual consistency.

Prefer iterative improvements over rewriting the entire frontend without justification.

Use screenshots or visual inspection when available. Review the actual rendered interface, not just the source code.

## 14. Final Design Principle

**Lyra should feel like a reference-grade audio component translated into software.**

The best premium Hi-Fi equipment does not impress through an abundance of controls, lights, or decoration.

It impresses through precision, restraint, craftsmanship, and confidence.

Every interaction should feel intentional.

Every component should belong.

Every detail should be considered.

**Make Lyra beautifully simple to use, not simple in what it can do.**
