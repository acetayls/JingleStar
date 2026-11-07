# CueStar help

CueStar sends videos, images and sounds to an output screen (projector, stage screen, video
desk). The control screen, where you work, and the **output**, which shows the picture, run as
two separate programs: a busy control screen never delays a frame.

Three modules share the screen: the **Player** (Lecteur, a playlist), the **Slideshow**
(Diaporama, a folder of photos and videos) and the **Cuelist** (a running order of cues fired
with GO). The **Text** panel sits alongside to keep the written running order in view.

## Getting started

1. At startup the output opens full screen on the second display (in a window if there is only
   one). The bottom bar reads **Sortie : 1920×1080 @ 60.00 Hz** once it is ready.
2. In the **Lecteur**, click **Ajouter** (Add) or drop files on the list.
3. Double-click an item, or select it and press **Space** (GO).
4. **Esc** or the red **NOIR** button cuts the picture at any time.
5. Right-click an item → **Edit…** to set its in and out points, fades to black, volume and
   equalizer.

Everything is kept automatically: when CueStar starts again, the session is as you left it.
**Enregistrer** (Save) writes the project to a `.cuestar` file.

Formats: videos (MP4, MOV, MKV, AVI, WebM, MXF, MPEG, TS, WMV…), images (JPEG, PNG, BMP, GIF,
TIFF, WebP) and sounds (MP3, WAV, FLAC, AAC/M4A, OGG). Decoding goes through libmpv.

## The screen

### Top bar

| Item | Role |
|---|---|
| Project name | Menu: New, Open, Save as, Rename |
| **Enregistrer** | Saves the project (**Ctrl+S**) |
| **Lecteur**, **Diaporama**, **Cuelist** | Switches module; a hidden module keeps playing |
| **Texte** | Shows or hides the Text panel |
| **? Help** | Opens this help (**F1**) |
| **Video output** | Output screen and engine, sound, test patterns, 4-corner mapping |
| **Monitoring** | Floating window showing the picture sent to the output |
| **Télécommandes** | OSC, MIDI, Yamaha and Midas consoles, mappings, shortcuts |
| **NOIR** | Cuts the picture: output to black (**Esc**) |

### Bottom bar

- **Sortie** (output): green when the picture goes through libmpv, orange when libmpv is
  missing (the output then shows a test pattern), red when the output stopped (**Relancer la
  sortie** restarts it).
- mpv version and current decoder (GPU or CPU).
- **Dropped / delayed frames**: frames the output could not show on time, added up over every
  media since the last reset. Hover for details. To start again from zero: the **↺** button, a
  right-click on the counter → **Reset**, the OSC address `output/drops/reset` or a remote.
- Media frame rate, CPU and GPU load.

## The Player

A playlist of videos, images and sounds. **Ajouter** opens files or a folder; you can also drop
them on the list. **Vider** clears the list (after confirmation).

- **GO** (or **Space**) plays the selected item, or resumes after a pause.
- **⏮ / ⏭**: previous / next item. **■** cuts the picture.
- **Up / Down arrow**: changes the selected item without playing it.
- **Aléatoire** (shuffle) plays in random order, **Tout boucler** (loop all) starts over at the
  end of the list.
- The loop icon at the end of a row loops that item.
- **Transition**: **Coupe** (cut), **Noir** (fade to black then up) or **Fondu enchaîné**
  (crossfade), with its duration.

### NEXT preview

On the right, the preview shows the item the next GO will play. You can move around in it
without touching the output: click or drag on the bar under the picture, **▶** plays the
preview (silently), **⏮ / ⏭** step one frame back or forward. The item's in and out marks are
drawn on the bar. **Aperçu** (above the list) shows or hides it.

### Right-click on an item

| Command | Effect |
|---|---|
| **Lire** | Plays the item |
| **Edit…** | Opens the item editor (see [Editing an item](#editing-an-item)) |
| **File information…** | File details, like MediaInfo (see [File information](#file-information)) |
| **Lire en boucle** / **Lire une seule fois** | Loops the item, or plays it once |
| **Retirer** | Removes the item from the list |

An item set in the editor carries the **✎** mark.

## Editing an item

Right-click → **Edit…**, on a Player item or on a Cuelist cue. The window follows JingleStar's
jingle editor. Settings are saved with the project.

- **Volume** of the item's sound, in dB, and play **once** or **looped**.
- **Fades to black**: length of the fade in (from black) and of the fade out (to black),
  picture and sound together.
- **Strip**: thumbnails of the media over its whole length. Drag the **in** and **out** handles
  to trim the start and the end; fades show as a veil on the strip. While dragging, the preview
  shows the picture at that point.
- **Preview**: the same navigation as the NEXT preview, to check an in point to the frame.
- **Equalizer**: low cut (12 or 24 dB/oct), low shelf, two bell bands, high shelf, as in
  JingleStar. The sound's waveform shows behind the curve.
- **Play on the output** plays the item to try it. If the item is already on air, volume,
  equalizer and fades change at once, without cutting the picture.

### Still images

For an image, the editor offers the **display time**: **Until the next media** (the image stays
until the next GO) or **For** a number of seconds, after which the Player moves on. A fade out
on an image needs a display time.

## File information

Right-click a Player item or a cue → **File information…**. The window details the container
and every video, audio and subtitle track: codec, profile, bit rate, resolution, frame rate,
scan type, colour space and range, channels, sample rate, language, metadata.

- **Search** filters the rows (for example "bit rate" or "HDR").
- **Show everything** switches to MediaInfo's complete view: every field, in every form.
- **Copy** puts the report on the clipboard as text, to send to a technician.

The report comes from the MediaInfo library when it is installed (`MediaInfo.dll` in CueStar's
`mpv` folder on Windows). Without it, CueStar shows what libmpv knows about the file. The file
is read in a separate program: a damaged file cannot crash the control screen.

## The Slideshow

Plays a folder of photos and videos.

- **Dossier** (folder): the folder to play; **Relire** picks up added or removed files.
- **Include subfolders**, **photo duration**, **fit** (whole picture with black bars, fill the
  screen by cropping, or automatic) and **videos with sound** (played in full).
- **Space** plays or pauses, **← / →** go to the previous / next media.
- **Aléatoire** reshuffles on every round, **Tout boucler** starts over.

Changes apply at once, without restarting the slideshow.

## The Cuelist

A running order of cues: each one is a media, or **black**. The **standby** cue plays on GO.

- **Add**: media, a folder, or **Un cue Noir** (a black cue, cut with the cue's transition).
- **GO** (or **Space**) plays the standby cue and puts the next one on standby.
- **⏮ / ⏭** (or **Up / Down arrow**) change the standby cue without playing it.
- **Pause** (or **P**) pauses and resumes the playing cue.
- Click a cue: puts it on standby. Double-click: plays it. Drag a cue to reorder.
- Right-click: **Lancer maintenant** (play now), **Réglages…** (settings), **File
  information…**, **Mettre en attente** (standby), **Retirer de la conduite** (remove).

**Réglages…** opens the cue editor: its name, its media, its **transition** (cut, black,
crossfade), what happens **at the end** (wait for GO, or go on to the next cue), then the same
settings as the [item editor](#editing-an-item).

## The Text panel

The written running order, next to the current module: bold, italic, underline, strikethrough,
colours, size. **Importer…** brings in an existing text. It is saved with the project.

## Video and sound output

**Video output** opens the output settings:

- **Output screen**: automatic (first secondary screen) or a given screen; **output in a
  window** when there is only one screen.
- **Decoding**: automatic, GPU or CPU. **Fallback renderer** and **light rendering** for a small
  machine.
- **Cadence**: speed resync or audio sync; **Interpolation** smooths motion when the video frame
  rate does not fit the screen's; **Match the screen refresh rate to the video** (Windows).
- **Audio output**: sound card, HDMI of the output screen, Dante interface… and its volume.
- **Test patterns**: SMPTE bars, moving pattern, grid, white, grey.
- **4-corner mapping**: move the picture's corners to fit a surface; arrows = 1 pixel,
  Shift + arrows = 10 pixels.

Changes that need the output to restart say so: the screen goes black for a moment.

## Projects and saving

- The session (lists, settings, text) is kept continuously and restored at startup.
- **Ctrl+S** saves the project, **Ctrl+Shift+S** saves it elsewhere, **Ctrl+O** opens one,
  **Ctrl+N** starts a blank project (after confirmation).
- If media were moved, a banner says so: **Relocaliser…** looks for them by name in a folder.
- Machine preferences (output screen, decoding, sound, remotes) do not travel with the project.

## OSC control

CueStar can be driven remotely over OSC (QLab, TouchOSC, Chataigne, Companion, a script…), with
the same settings as JingleStar.

### Enable

In **Télécommandes**, **OSC** tab, tick **Enable**, set the options, then click **Apply**:

| Setting | Default | Role |
|---|---|---|
| Access | Local | **Local**: from this PC only; **Network**: from other machines |
| Port (UDP) | 9000 | Port CueStar receives commands on |
| Prefix | `/cuestar` | Start of every address |
| OSCQuery | off, port 9001 | HTTP server describing the addresses and the current state |
| Discovery | off | mDNS / Bonjour announcement for automatic discovery |
| Advertised name | CueStar | Name shown on the network |

Once applied, the window shows the IP address and listening ports, or the error if a port is
taken, then the last message received.

### Addresses

Numbers start at 1. Addresses are given with the default prefix; the original Diaporama
addresses (`/diaporama/…`) are accepted too.

| Address | Argument | Effect |
|---|---|---|
| `/cuestar/noir` | — | Output to black |
| `/cuestar/output/drops/reset` | — | Resets the dropped and delayed frame counter |
| `/cuestar/lecteur/go` | — | Plays the selected item (or resumes) |
| `/cuestar/lecteur/play` | — | Play |
| `/cuestar/lecteur/pause` | — | Pause |
| `/cuestar/lecteur/toggle` | — | Pause / resume |
| `/cuestar/lecteur/stop` | — | Stop (black) |
| `/cuestar/lecteur/next` | — | Next item |
| `/cuestar/lecteur/prev` | — | Previous item |
| `/cuestar/lecteur/goto` | int | Plays item number n |
| `/cuestar/lecteur/shuffle` | bool | Shuffle |
| `/cuestar/lecteur/loop` | bool | Loop all |
| `/cuestar/lecteur/transition` | `cut`, `black` or `fade`, then float (s) | Transition and duration |
| `/cuestar/cuelist/go` | — | Plays the standby cue (or resumes) |
| `/cuestar/cuelist/play`, `pause`, `toggle`, `stop` | — | Cuelist transport |
| `/cuestar/cuelist/next`, `prev` | — | Next / previous standby cue |
| `/cuestar/cuelist/standby` | int | Puts cue number n on standby |
| `/cuestar/diaporama/play`, `pause`, `toggle`, `stop` | — | Slideshow transport |
| `/cuestar/diaporama/next`, `prev` | — | Next / previous media |
| `/cuestar/diaporama/goto` | int | Media number n |
| `/cuestar/diaporama/settings/shuffle`, `loop`, `recursive`, `video_sound` | bool | Slideshow settings |
| `/cuestar/diaporama/settings/photo_duration` | float (1 to 60 s) | Photo duration |
| `/cuestar/diaporama/settings/fit` | `contain`, `cover` or `auto` | Fit |
| `/cuestar/diaporama/settings/folder` | string | Folder to play |

- Triggers fire with no argument or a non-zero value: a button sending 1 on press and 0 on
  release fires only once.
- Booleans accept `T`/`F`, an int or a float (≥ 0.5 = true), or `true`/`false`/`on`/`off`.
- Bundles are accepted.

### State feedback

`/cuestar/subscribe` (argument: reply port, optional) subscribes the sender for 5 s: renew it
every 2 to 3 s. CueStar sends it `media`, `paused`, `time`, `remaining`, `lecteur/playing`,
`cuelist/standby`, `cuelist/standby_name` and `diaporama/index`.

### OSCQuery

- `http://<ip>:9001/` returns the address tree as JSON (types, access, ranges and current
  values).
- `http://<ip>:9001/?HOST_INFO` returns the server information.
- `http://<ip>:9001/<path>?VALUE` returns one node's value.

Read-only nodes also give the media on screen, the position, the remaining time, the number of
items and cues, the playing item and the standby cue's name.

### Example

```
oscsend localhost 9000 /cuestar/lecteur/goto i 3
oscsend localhost 9000 /cuestar/cuelist/go
oscsend localhost 9000 /cuestar/lecteur/transition sf black 1.5
curl http://localhost:9001/cuestar/cuelist/standby_name?VALUE
```

## MIDI and console control

**Télécommandes** links MIDI controllers, a Yamaha console or a Midas M32 / Behringer X32 to
CueStar's actions, as in JingleStar:

1. Connect the source in the **MIDI**, **Yamaha** or **Midas M32 / X32** tab.
2. In **Mappings**, add a row, pick the action, click **Learn**, then touch the note, the
   controller or the console key.

Available actions: GO, pause, stop, next, previous, item number (from the value received),
shuffle and loop all for the Player; play, pause, stop, next, previous for the Slideshow; GO,
next cue, previous cue, pause, stop for the Cuelist; **Black**; dropped frame counter reset.

## Keyboard and mouse shortcuts

The key list is also in **Télécommandes → Shortcuts**. Single keys do nothing while you type
in a text field.

| Action | Shortcut |
|---|---|
| Black: cut the output | **Esc** |
| Open or close help | **F1** |
| Player or Cuelist GO; Slideshow play / pause | **Space** |
| Cuelist pause / resume | **P** |
| Previous / next standby item or cue | **Up / Down arrow** |
| Slideshow previous / next media | **Left / Right arrow** |
| Save / Save as | **Ctrl+S** / **Ctrl+Shift+S** |
| Open a project | **Ctrl+O** |
| New project | **Ctrl+N** |
| Text: bold / italic / underline | **Ctrl+B** / **Ctrl+I** / **Ctrl+U** |
| Edit an item, file information | Right-click the item |
| Play an item or a cue | Double-click |
| Move around in the preview | Click or drag on the bar under the picture |
| Search the help | Ctrl+F in the help window |

## Troubleshooting

- **The output shows a test pattern instead of the media**: libmpv was not found. Put
  `libmpv-2.dll` next to CueStar or in its `mpv` folder, then restart.
- **Output stopped** (red light): **Relancer la sortie** in the bottom bar.
- **Dropped or delayed frames keep rising**: try **GPU** decoding, **light rendering** or
  **Match the screen refresh rate to the video** in **Video output**; **File information**
  shows whether the media is very heavy (bit rate, resolution, 10-bit).
- **No sound**: pick the card in **Video output → Audio output**.
- **Media not found**: **Relocaliser…** in the banner.
- **Odd behaviour or a crash**: the `cuestar.log` journal, in the configuration folder, keeps
  errors and crash traces.
- **OSC does not respond**: check that control is enabled and applied, the access (Local or
  Network), the port, the address prefix, and that the firewall lets the UDP port through.
