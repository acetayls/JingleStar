# JingleStar help

JingleStar plays jingles and playlists on one or more audio outputs. The screen is split into
**modules** (1 to 4): each one is either a grid of 12 jingles or a playlist, with its own volume,
equalizer and output pair.

## Getting started

1. On first launch, a single module in **Jingles** mode is shown.
2. Click an empty button: a dialog lets you pick a sound file. You can also **drag and drop** one or
   more files onto a button (several files fill the following buttons).
3. Click a filled button to play the sound, click it again to stop it.
4. **Esc** or the red **STOP ALL** button stops every sound.
5. Check the audio output in the bottom bar. To change it: **Preferences → Audio**.
6. On the right of the bottom bar, **● All loaded** (green) means every jingle and cue sound is
   decoded in memory and will start instantly. While loading, the light is amber and counts the
   ready sounds; **⚠** (red) flags a missing or unreadable file (hover for its name). Next to it,
   the memory used by decoded sounds (about 23 MB per minute of sound).

Everything is saved automatically: when you restart JingleStar, the session is restored as you
left it.

Supported formats: WAV, AIFF, MP3, FLAC, OGG/Vorbis, AAC/M4A.

## The screen

### Top bar

- **＋ Add module**: adds a module at the end (4 maximum). The counter shows the number of modules.
- Modules sit side by side in one row. **Drag the divider** between two modules to set their width; **double-click** a divider to make all widths equal again. Widths are saved with the project.
- **New**: blank project with one **Jingles** module and one **Playlist** module. Click **Replace the project?** to confirm (**Cancel** otherwise); preferences (audio, remote control, language) are kept. Save the current project first if you want it back.
- **Open…** / **Save as…**: opens or saves a project (`.jinglestar` file).
- **Remote**: OSC, MIDI, Yamaha console and Midas M32 control, mappings and learn mode. See [MIDI control](#midi-control), [Control from a Yamaha console](#control-from-a-yamaha-console) and [Control from a Midas M32 / Behringer X32](#control-from-a-midas-m32--behringer-x32).
- **⚙ Preferences**: audio, language and sleep prevention.
- **? Help** (**F1** key): this manual. The **🔍 Search** field at the top (or **Ctrl+F**) shows only the passages containing the typed words, ignoring accents and case; **✕** goes back to the contents.
- **■ STOP ALL**: stops every sound in every module (**Esc** key).

### Bottom bar

It shows the active audio output (driver, device, channel count, sample rate) with a green light,
or a red light when there is no output. Errors and messages (project saved, unreadable file…) are
shown there too.

## Modules

Each module has, from top to bottom:

- its **number** and **name**;
- the **output** selector (stereo pair 1-2, 3-4…);
- the **EQ** button (module equalizer);
- the **✏** button (edit mode);
- the mode selector: **Jingles**, **Playlist**, **Cuelist** or **Text**;
- on the right, the volume **fader** (-∞ to +10 dB) with marks at +3, 0 and -6 dB and its dB value below, the stereo **meter** and the **■** button that stops the module.

The module frame turns green while something is playing in it.

- **Volume**: drag the fader. A **double-click** resets it to 0 dB. **Click the dB value** to type it on the keyboard (Enter confirms, Escape cancels). A number typed without a sign is negative: `6` gives -6 dB; type `+3` to go above 0 dB.
- **Change mode**: click **Jingles**, **Playlist**, **Cuelist** or **Text**. The module stops; the jingles, the playlist, the cue list and the text are all kept.
- **Rename**: **double-click the module name**, type the new name and press Enter (Escape cancels). In edit mode (✏) the name is also directly editable.
- **Text module**: a large free text field to write your running order, saved with the project. Escape still stops the jingles from there.
- **Delete**: in edit mode, click **🗑** then confirm with **Delete?**. The following modules move up one place. The last remaining module cannot be deleted.

### Edit mode (✏)

The pencil switches the module between **play** mode and **edit** mode:

- in play mode, clicking a jingle plays or stops it;
- in edit mode, clicking a jingle opens its editor (and asks for a file if the button is empty), the module name becomes editable, and the **EQ** and **🗑** buttons appear; in Playlist mode, the transition and level normalization settings are shown as well.

When you close a jingle editor (**✓ Done** or the close cross), the module goes back to play mode.

## Jingles

A module in **Jingles** mode has 12 buttons (3 columns, 4 rows). Each button shows its number, the
sound name, its color and its duration. While playing, a progress bar shows elapsed and remaining
time; a looping jingle shows **LOOP** instead of the remaining time. A playing jingle has a thick
green frame. The jingle volume is shown at the top right (orange when it is not 0 dB). Extra marks
complete the button: at the top left, after the number, an orange **⟲** badge for a looping jingle and **EQ** when
its equalizer is on;
**…** while the sound is loading and **⚠** if the file is missing or unreadable.

- **Click**: plays the sound from its start point; a second click stops it with its fade-out (capped at 1 s for a one-shot jingle, in full for a looping one; a loop stopped during its intro, before reaching the loop, cuts at once).
- Several jingles of the same module can play at the same time.
- **Right-click**: **Choose a file…**, **Edit…** (opens the editor without entering edit mode) and **Clear**.
- **Drag and drop** a file onto a button to replace its sound; several files fill the following buttons of the module.

### Editing a jingle

Open the editor through edit mode (✏ then click the button) or with **right-click → Edit…**.

- **Name**: the text shown on the button (the file name by default).
- **File**: **Change…** to pick another sound.
- **Volume**: from -40 to +10 dB, in 0.5 dB steps.
- **Playback**: **Once** or **Loop**.
- **Fades**: fade-**in** and fade-**out** duration, from 0 to 30 s.
  - Played **once**: the sound rises at the start and falls before its end. Stopped early, it dies away with its fade-out, capped at 1 s.
  - **Loop**: the fade-in only plays on the first start, not on every pass. The fade-out only applies on stop, in full: that is how a background bed slowly disappears. Stopped during the intro (before the loop start), it cuts at once.
  - **Escape**, **STOP ALL** and the module's **■** always cut within 1 s.
- **Color**: 8 colors to choose from.
- **Play / Stop**: the big green button at the top of the editor, to try the sound.
- **Clear slot**: removes the sound from the button.
- **✓ Done**: closes the editor.

Changes apply immediately, even while playing. If the window is taller than the screen, a scroll bar appears on the right.

### Waveform, start, end and loop

The editor shows the sound's waveform with draggable handles, QLab style:

- **green** handle (bottom): playback **start**;
- **red** handle (bottom): playback **end**;
- **orange** lines (top), in loop mode: **loop start** and **loop end**.

The **fade in** and **fade out** are set with the **white squares** in the band above the
waveform: drag them inwards to lengthen the fade. The duration is shown next to them and the
waveform is drawn **attenuated** over the whole fade, as it will sound, with the original sound
faintly behind it (when looping, the fade out only applies on stop and is set in its field).

**Zoom**: the mouse wheel (or Ctrl + wheel, or a trackpad pinch) zooms around the pointer;
horizontal scrolling or dragging away from the handles moves the view; a double-click or
**Show all** returns to the whole sound. When zoomed in, a **scroll bar** under the waveform shows
the visible part: drag it to travel through the sound, or click beside it to jump there.

While playing (jingle button or **Play**), a **click on the waveform**, away from the handles,
moves playback to that spot, with a click-free micro-fade.

Excluded parts are darkened and fades are drawn as ramps. Start and end are grabbed in the lower
half of the waveform, loop points in the upper half, so they can be told apart even when they
overlap.

Below the waveform, the **Start**, **Loop → …** and **End** fields let you type the points to a
hundredth of a second. **Reset points** goes back to the whole sound.

In loop mode, the sound plays from the start to the loop end, jumps back to the loop start, and so
on until you stop it.

### Jingle equalizer

At the bottom of the editor, the collapsible **Equalizer** section sets an equalizer for this
jingle only. It turns orange and reads “on” when it changes the sound. See
[The equalizer](#the-equalizer).

## The playlist

A module in **Playlist** mode plays a list of audio files: a whole folder, sorted by name, or
files picked one by one.

- **＋ Add**:
  - **Files…** or **A folder…**: adds the sounds at the end of the current list;
  - **Open a folder (replaces the list)…**: the list becomes the folder's content.
  You can also drop files or a folder onto the playlist.
- **🗑 Clear**: removes every track after a confirmation (**Clear?** to confirm, **Cancel** otherwise). Files are not deleted.
- **⟳**: rescans the folder (after adding or removing files).
- The top frame shows the current track, a progress bar, elapsed and remaining time.

### Transport

- **⏮** / **⏭**: previous / next track (playback continues if it was running).
- **▶ GO**: starts playback at the selected track, or the first one. When paused, resumes playback. After **■**, restarts the stopped track from the beginning; once the list has ended, starts again from the first track. The sound is prepared in advance, so it starts instantly.
- **⏸**: pause. The track fades out in 10 ms and resumes at exactly the same spot, click-free, with ⏸ or GO. The button stays lit in amber and "PAUSED" is shown while paused.
- **■**: stops the playlist.
- **Progress bar**: click or drag to move within the current track (applied on release, with a micro-fade to avoid clicks).
- **Click** a track: selects it (when stopped). **Double-click**: plays it right away.
- **Track gain**: the dB value on the right of each track can be set by hand with a
  **click** (Enter confirms, Escape cancels). A manual gain is shown in blue and replaces the
  automatic level balancing; the current track takes it at once. Right-click the track →
  **Back to automatic gain**.

### Options

Tracks always advance automatically. An amber light shows which options are on.

- **Shuffle**: random order, without repeating a track before the whole list has played.
- **Loop all**: starts over at the end of the list.

### Transition

The **Transition** row offers:

- **End of file**: the next track starts exactly when the previous one ends, gapless;
- **Crossfade**: the next track fades in while the previous one fades out, over an adjustable time from 0.5 to 20 s (3 s by default).

### Level normalization

To make every track play at the same level, tick **Normalize level** under the transition. The
target and measuring mode are set in edit mode (✏). Each
track is measured in the background, then played with the gain that brings it to the target. Files
are never modified.

- **Measurement**: **Loudness (LUFS)** (integrated loudness, ITU-R BS.1770 / EBU R128, closest to what the ear hears), **Peak**, **True peak** (dBTP) or **RMS**.
- **Target**: the level to reach. -16 LUFS by default; for reference: -23 LUFS for radio and TV (EBU R128), -14 to -16 LUFS for streaming and live sound.
- **Max** (on by default, -1 dBTP): if the gain would push a track's true peak above this ceiling, its gain is lowered so nothing clips.

The applied gain is shown to the right of each track and next to the current track: grey normally,
**orange** when lowered by the ceiling, **…** while measuring, **—** for a silent or too short
track (gain unchanged), **⚠** if the measurement failed. Hovering shows the detailed measurements
(loudness, peak, true peak, RMS). While measuring, an indicator shows the progress. Measurements
are cached and only redone when the file changes.

Outside edit mode, a **Normalize** row reminds you of the current setting.

## The Cuelist (cue list)

**Cuelist** mode follows a show running order, QLab style: an ordered list of sounds (the **cues**)
fired one by one.

- **Add sounds**: **＋ Add sounds…**, or drop files or folders onto the list. A playlist track can
  also be dragged in, at the place you want.
- **RUNNING**: the green frame at the top follows the last fired cue still playing: its name,
  elapsed time, a progress bar and remaining time.
- **STANDBY**: the amber frame shows the cue the next GO will fire, and the one after. In the list
  it is highlighted and marked ▶.
- **GO** (or the **space bar**): fires the standby cue and puts the next one on standby. Fired cues
  keep playing together until they end. The double-GO protection from the preferences also guards
  this button. After the last cue the frame shows **End of the cue list** and GO goes dark.
- **Previous** / **Next**: move the standby cue without firing anything. **Click** a cue to put it
  on standby.
- **Stop**: stops every cue of the list; the standby cue does not change. While a cue plays, its row
  fills in green, shows the remaining time and a **■** button that stops only that cue.
- **Continue or stop**: the small arrow next to a cue's number. **↓** (orange): when the cue
  ends, the next one starts by itself and the standby cue moves on. **·**: when it ends, wait for
  the next GO. A click toggles. A cue stopped by hand (■, Stop, Esc) does not continue.
- **Cue settings**: **double-click** the cue, or **right-click → Settings…**. The window is the
  jingle one: volume, fade in and out, start and end points on the waveform (click the waveform
  while playing to move there), equalizer, and the **When it ends: Stop / Continue** choice. No
  loop for cues for now. An **EQ** badge on the row means its equalizer is on.
- **Volume**: **click** a cue's dB value to type it (Enter confirms; without a sign the value is negative).
- **Order**: drag a cue up or down; a blue line shows where it will land. The standby cue stays the
  same sound.
- **Right-click** a cue: **Play now** (without changing the standby cue), **Settings…**, **Put on standby**,
  **Remove from the list**.
- In **edit mode** (✏), cue names are directly editable and **🗑** clears the list.

The space bar drives the first Cuelist on screen (except while typing text). Remote controls have
the **Cue list: GO**, **Cue list: next cue** and **Cue list: previous cue** actions, and OSC has the
`go` and `cue/…` addresses (see [OSC control](#osc-control)).

## The equalizer

Every jingle and every module has its own equalizer. The module equalizer applies to everything the
module plays (jingles and playlist), after the jingle's own.

- **Module equalizer**: in edit mode, **EQ** button in the module header. Outside edit mode, a small orange **EQ** label shows that it is on.
- **Jingle equalizer**: **Equalizer** section of the jingle editor.

It has five bands:

| Band | Settings |
|---|---|
| Low cut | frequency, 12 or 24 dB/oct slope |
| Low shelf | frequency, gain, Q |
| Peak 1 | frequency, gain, Q |
| Peak 2 | frequency, gain, Q |
| High shelf | frequency, gain, Q |

Gain ranges from -18 to +18 dB, frequency from 20 Hz to 20 kHz, Q from 0.1 to 18.

### With the mouse, on the curve

- **Drag a point**: moves the frequency (horizontally) and the gain (vertically), with a slight detent at 0 dB.
- **Mouse wheel** over a point: widens or narrows the band (Q).
- **Double-click** a point: turns the band on or off.

### With the fields

Below the curve, each band has a button (on / off) and **Freq.**, **Gain** and **Q / slope** fields
to drag or type into. A click (or double-click) on a value lets you type it: Enter confirms, Escape cancels. Changing a value turns the band on.

- **EQ enabled**: master switch. When off, the sound passes untouched but the settings are kept (the curve stays visible, dimmed).
- **Reset**: back to a flat equalizer.

## Output routing

Each module plays on a **stereo pair**: 1-2, 3-4, 5-6… Pick it from the **Output** menu in the
module header.

- With a stereo sound card, only pair 1-2 exists: all modules are mixed there.
- With a multichannel interface, give each module its own pair for multitrack playback (for example jingles on 1-2 and background music on 3-4).
- If the chosen pair does not exist on the active device (interface unplugged, another one selected), the menu shows **⚠ Output x-y → 1-2** in orange: the module then plays on 1-2, and gets its pair back once it is available again.

## Preferences

**⚙ Preferences** opens the settings window. **Audio** settings take effect with
**Apply**, which also closes the window; language and sleep prevention apply right away.

### Audio

- **Driver**: WASAPI (Windows), CoreAudio (macOS), ALSA (Linux), or ASIO if JingleStar was built with that option. “Default” uses the system driver.
- **Device**: the output to use, with its channel count. ★ marks the system default output.
- **Buffer size**: Automatic, or 64 to 2048 samples. Smaller = less latency, but more risk of crackles.
- **Refresh list**: rescans drivers and devices (after plugging in an interface).

Applying an audio change reopens the device, which interrupts the sounds that are playing. If
nothing changed in the audio settings, playback is not interrupted.

### System

- **Language**: Français or English. The interface and this help switch immediately.
- **Prevent sleep during playback**: while a sound is playing (or paused), the computer and screen do not go to sleep. On Windows, JingleStar asks the system (see `powercfg /requests`); on macOS, through a power assertion (see `pmset -g assertions`). Closing a laptop's lid still puts it to sleep.

At the bottom of the window, the **build version** (version, commit and build date) tells you which
update is installed; click it to copy it.

## Projects and saving

- The session is **saved automatically** and restored on next launch (modules, jingles, playlists, preferences).
- **Save as…** creates a `.jinglestar` file (a project), to keep several setups.
- **Open…** loads a project: every sound stops, then the project replaces the current session.

A project stores the **path** of the sound files, not the sounds themselves: if you move or rename
a file, the matching button shows an error. Pick the file again (**Change…** or right-click →
**Choose a file…**).

## OSC control

JingleStar can be remote-controlled over OSC (for example from QLab, TouchOSC, Chataigne, a control
surface or a script).

### Enabling it

In **Remote**, **OSC** section at the top of the window, tick **Enable OSC control**, adjust the options and click **Apply**:

| Setting | Default | Purpose |
|---|---|---|
| UDP port (receive) | 8000 | Port on which JingleStar receives commands |
| Address prefix | `/jinglestar` | Start of every address (may be empty) |
| OSCQuery | on, port 8001 | HTTP server describing the available commands |
| Network discovery | on | mDNS / Bonjour advertisement for automatic discovery |
| Advertised name | JingleStar | Name shown on the network |

Once applied, the preferences show the IP address and listening ports, or the error if a port is
already in use.

### Addresses

Module (1 to 4), jingle (1 to 12) and track numbers start at 1. Addresses are given with the
default prefix.

| Address | Argument | Effect |
|---|---|---|
| `/jinglestar/stop_all` | — | Stops everything |
| `/jinglestar/module/<m>/stop` | — | Stops the module |
| `/jinglestar/module/<m>/volume` | float, dB (-60 to +10) | Module volume |
| `/jinglestar/module/<m>/mode` | `jingle`, `playlist`, `text` or `go` | Changes the module mode |
| `/jinglestar/module/<m>/jingle/<j>/play` | — | Plays the jingle from the start (restarts it if playing) |
| `/jinglestar/module/<m>/jingle/<j>/stop` | — | Stops the jingle |
| `/jinglestar/module/<m>/jingle/<j>/toggle` | — | Plays or stops, like a click |
| `/jinglestar/module/<m>/jingle/<j>/volume` | float, dB (-40 to +10) | Jingle volume |
| `/jinglestar/module/<m>/playlist/play` | — or int | Play (selected track, or track no.) |
| `/jinglestar/module/<m>/playlist/track` | int | Plays track no. |
| `/jinglestar/module/<m>/playlist/stop` | — | Stops the playlist |
| `/jinglestar/module/<m>/playlist/pause` | — | Pause / resume |
| `/jinglestar/module/<m>/playlist/paused` | bool | Pauses (1) or resumes (0) |
| `/jinglestar/module/<m>/playlist/next` | — | Next track |
| `/jinglestar/module/<m>/playlist/previous` | — | Previous track |
| `/jinglestar/module/<m>/playlist/shuffle` | bool | Shuffle |
| `/jinglestar/module/<m>/playlist/loop` | bool | Loop the list |
| `/jinglestar/go` | — | GO of the first Cuelist |
| `/jinglestar/module/<m>/go` | — | GO: fires the standby cue, moves to the next |
| `/jinglestar/module/<m>/cue/next` | — | Next cue on standby |
| `/jinglestar/module/<m>/cue/previous` | — | Previous cue on standby |
| `/jinglestar/module/<m>/cue/standby` | int | Puts cue no. on standby |

- Triggers (`play`, `stop`, `toggle`, `next`…) fire with no argument or a non-zero value: a button sending 1 on press and 0 on release triggers only once.
- Booleans accept `T`/`F`, an int or a float (≥ 0.5 = true), or `true`/`false`/`on`/`off`.
- Jingle commands only act on a module in Jingles mode, playlist commands on a module in Playlist mode. Bundles are accepted.

### OSCQuery

- `http://<ip>:8001/` returns the command tree as JSON (types, access, ranges and current values).
- `http://<ip>:8001/?HOST_INFO` returns the server information.
- `http://<ip>:8001/<path>?VALUE` returns the value of a node.

The tree follows the project state: number of modules, mode of each, jingle names. Read-only nodes
are also available: `name`, `jingle/<j>/name`, `jingle/<j>/playing`, `playlist/playing` and
`playlist/tracks`. The service is advertised as `_oscjson._tcp` and `_osc._udp`: Chataigne,
TouchOSC, Vezér or Ossia Score find it on their own. Live notifications (WebSocket `LISTEN`) are
not offered: clients read values again over HTTP.

### Example

```
oscsend localhost 8000 /jinglestar/module/1/jingle/3/play
oscsend localhost 8000 /jinglestar/module/2/volume f -6
curl http://localhost:8001/jinglestar/module/2/volume?VALUE
```

## MIDI control

JingleStar can be driven by a keyboard, a pad or fader controller, or a console's MIDI output
(for example a Yamaha QL through a USB-MIDI interface). Notes, CCs and program changes trigger
the same actions as OSC.

### Connecting

Click **Remote** in the top bar, tick **Enable MIDI input** and pick the port (or “First
available port”). Settings apply immediately. If the controller is unplugged, JingleStar
reconnects it on its own when it comes back; **⟳** rereads the port list.

### Learning a mapping

1. Click **＋ Add and learn**: a new line appears, waiting.
2. Press the key or pad, or move the controller's fader: it is assigned to the line.
3. Choose the action, the module and, for a jingle, its number.

**Learn** on an existing line reassigns it. The **✏** button next to the learned value lets you
correct it by hand: type (note, CC, program change, console key or scene), channel, note or
controller number, console channel. You can even build a mapping without the controller at hand.

 A new line copies the previous line's action, on
the next jingle: for a 12-pad controller, repeat **＋ Add and learn** then the next pad.
**Last message received** shows what comes in, to check the connection.

### How messages act

| Message | Buttons (play, stop, next…) | Volumes | Shuffle, loop | Track no. |
|---|---|---|---|---|
| Note | on press (release is ignored) | velocity | toggles | — |
| CC | when the value reaches 64 or more | 0 → minimum, 127 → maximum | 64 or more = on | value |
| Program change | on every message | — | toggles | program number |

For **Playlist: track no.** on a program change, every program change on the learned channel
picks the track: program 1 → track 1, program 2 → track 2…

Jingle actions are ignored when the module is in playlist mode, and vice versa.

### Soft takeover of volumes

A fader (console or MIDI controller) assigned to a volume only takes over once it reaches
JingleStar's current volume, within 1 dB or by crossing it: no volume jump when the two are not
in the same place. Until then, the mapping line shows **bring the fader to … dB**. If the volume
is then changed with the mouse or over OSC, the fader has to catch it again.

## Control from a Yamaha console

JingleStar listens directly to a Yamaha console over the network (QL, CL, TF, DM7, Rivage), with
no extra hardware: connect the computer to the console's network. JingleStar only **listens**
to the console's changes; it never changes anything on it.

### Connecting

In **Remote**, **Yamaha console (network)** section, enter the console's **IP** (set on the
console in SETUP → NETWORK), keep port **49280** and click **Connect**. If the console is off
or unplugged, JingleStar retries every few seconds on its own. The **Received** line shows the
last message sent by the console and how long ago.

### Console in use

JingleStar writes **not a single byte** to a Yamaha console: it opens the connection and only
reads. JingleStar contains no command at all for these consoles, neither changes (`set`, scene
recall…) nor even information requests.

Before plugging the computer into the network of a console in use, give it a free IP address on
that network (an address already used by the console, a tablet or a stagebox would disturb
their link) and plug it into the control network port, never into the Dante ports.

### What can trigger an action

- **A channel ON key**: each press toggles ON and triggers the action. A spare channel (an
  unused input, for example placed in a custom fader bank) becomes a button this way. If your
  console lets you assign a **USER DEFINED** key to that channel's ON, the key becomes a
  JingleStar button.
- **A fader**: sets a module volume (so also the playlist volume of a module in playlist mode)
  or a jingle volume. The dB level shown on the console is used as is: 0 dB on the console =
  0 dB in JingleStar, so the Yamaha fader curve is kept; -∞ gives the minimum. For a button
  action (play, stop…), it fires when the fader leaves -∞.
- **A scene recall**: triggers the action of the learned scene. With **Playlist: track no.**,
  any recalled scene plays the track with the same number.

For playlist options (shuffle, loop), a channel ON sets the state: ON lit = option
on.

### Learning

As with MIDI: **＋ Add and learn**, then press the ON key, move the fader or recall the scene on
the console. The trigger is shown, for example **QL · InCh/Fader/On 32** (ON key of channel 32)
or **Console · Scene 12**.

## Control from a Midas M32 / Behringer X32

JingleStar also listens to Midas M32 and Behringer X32 consoles over the network (OSC, UDP port
**10023**), with the same mapping table and learn mode.

### Connecting

In **Remote**, **Midas M32 / Behringer X32 console** section, enter the console's **IP** (SETUP →
Network on the console), keep port **10023** and click **Connect**. The model is shown as soon
as the console answers, and the **Received** line shows its last message.

### What is sent to the console

Unlike Yamaha consoles, the M32 only sends its changes to clients that subscribe, and the
subscription lasts 10 seconds. Listening without sending anything is therefore impossible.
JingleStar only sends:

- `/xremote` every 8 seconds: the subscription to changes;
- `/info` at the same time: the model name and version.

Both requests have no argument and change nothing on the console. JingleStar contains no other
command for the M32. As with Yamaha consoles, give the computer a free IP address on the
console's network.

### What can trigger an action

- **MUTE key of a channel, bus or DCA** (`…/mix/on` parameter): each press triggers the action,
  like the Yamaha ON key. **Mute groups** (`/config/mute/1` to 6) work the same way. A spare
  channel becomes a button this way.
- **Fader**: sets a volume. The position is converted to dB with the M32 fader law (0 dB on the
  console = 0 dB in JingleStar), with soft takeover.
- **Scene recall**: triggers the learned scene's action; with **Playlist: track no.**, scene 1
  plays track 1, and so on.

The learned trigger is shown, for example, as **M32 · ch/05/mix/on** (MUTE key of channel 5).

## Keyboard and mouse shortcuts

| Action | Shortcut |
|---|---|
| Stop all | **Esc** (except while typing text) |
| Open or close the help | **F1** |
| GO of the first Cuelist | **Space** (except while typing text) |
| Reorder the cue list | Drag a cue |
| Play / stop a jingle | Click the button (play mode) |
| Jingle menu | Right-click the button |
| Fill buttons | Drag and drop one or more files |
| Add to a playlist | Drag and drop files or a folder onto the playlist |
| Play a playlist track | Double-click the track |
| Module volume to 0 dB | Double-click the fader |
| Type a dB value (fader, jingle, track) | Click the value, then Enter (Escape cancels); no sign = negative |
| Module widths | Drag the divider; double-click to equalize |
| Search the help | Ctrl+F in the help window |
| Rename a module | Double-click its name |
| Zoom the waveform | Wheel, drag to move, double-click to show all |
| Move within a playlist track | Click or drag the progress bar |
| EQ band width (Q) | Mouse wheel over the curve point |
| Turn an EQ band on / off | Double-click the curve point |
| Exact value in a number field | Click the field, type, then Enter (or drag to adjust) |

## Troubleshooting

- **No audio output** (red light): open **Preferences → Audio**, pick a device, click **Refresh list** if needed, then **Apply**.
- **Crackles**: increase the **buffer size** (512 or 1024 samples).
- **A button shows an error**: the file was moved, renamed, or its format is not supported. Pick the file again.
- **A module plays on the wrong output**: check its **Output** menu; an orange ⚠ means the pair does not exist on the active device.
- **OSC does not respond**: check that control is enabled and applied, that the port shown matches the sender's, the address prefix, and that the firewall lets the UDP port through.
