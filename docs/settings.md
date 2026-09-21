# Settings

LogiPeek stores its small set of user preferences in:

```text
%LOCALAPPDATA%\LogiPeek\settings.ini
```

The file contains plain `key=value` lines:

```ini
preset1=400
preset2=800
preset3=1600
preset4=3200
theme=system
language=en
startup=false
battery_notifications=true
battery_threshold=20
device=0123456789abcdef0123456789abcdef
```

The four presets accept positive unsigned 16-bit integers. The theme is `system`, `light`, or `dark`. The GUI language is `en` or `zh-CN`; it affects the window and tray but not CLI output. A missing or invalid language opens the in-window language picker before the normal settings page, including for upgrades from older settings. Other missing or malformed fields use their individual defaults, and unknown fields are ignored so that a damaged or newer settings file does not prevent startup.

`startup` defaults to `false`, while battery notifications default to enabled at `20%`. Thresholds accept 5 through 50 in five-point steps. The optional `device` value is a derived opaque identifier, never a raw HID path or serial number. A remembered selection is restored only when exactly one current candidate matches it; otherwise LogiPeek requires a new selection.

Preset values are preferences, not device profiles. A saved value remains visible after restart, but its button is enabled only when the selected device reports that exact DPI as supported. Saving settings never writes DPI to the mouse.

Language choices and normal settings saves write a sibling temporary file, flush it, and then replace `settings.ini` with the Win32 replace-existing and write-through flags. A save failure is shown in the window and leaves the application running. The Start with Windows switch separately manages only the `LogiPeek` value under the current user's Windows Run key; its displayed truth is reconciled with that real value instead of trusting the INI field. LogiPeek uses no database and never synchronizes settings over a network.
