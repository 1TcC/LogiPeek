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
```

The four presets accept positive unsigned 16-bit integers. The theme is `system`, `light`, or `dark`. The GUI language is `en` or `zh-CN`; it affects the window and tray but not CLI output. A missing or invalid language opens the in-window language picker before the normal settings page, including for upgrades from older settings. Other missing or malformed fields use their individual defaults, and unknown fields are ignored so that a damaged or newer settings file does not prevent startup.

Preset values are preferences, not device profiles. A saved value remains visible after restart, but its button is enabled only when the currently discovered device reports that exact DPI as supported. Saving settings never writes DPI to the mouse.

Language choices and normal settings saves write a sibling temporary file, flush it, and then replace `settings.ini` with the Win32 replace-existing and write-through flags. A save failure is shown in the window and leaves the application running. LogiPeek does not store settings in the registry, use a database, or synchronize them over a network.
