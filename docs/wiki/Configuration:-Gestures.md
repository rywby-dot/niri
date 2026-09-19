### Overview

<sup>Since: 25.02</sup>

The `gestures` config section contains gesture settings.
For an overview of all niri gestures, see the [Gestures](./Gestures.md) wiki page.

Here's a quick glance at the available settings along with their default values.

```kdl
gestures {
    dnd-edge-view-scroll {
        trigger-width 30
        delay-ms 100
        max-speed 1500
    }

    dnd-edge-workspace-switch {
        trigger-height 50
        delay-ms 100
        max-speed 1500
    }

    hot-corners {
        // off
        top-left
        // top-right
        // bottom-left
        // bottom-right
    }

    // Disabled unless these blocks are present.
    // hot-corners-expose { top-right; }
    // hot-corners-expose-all-outputs { bottom-right; }
}
```

### `dnd-edge-view-scroll`

Scroll the tiling view when moving the mouse cursor against a monitor edge during drag-and-drop (DnD).
Also works on a touchscreen.

This will work for regular drag-and-drop (e.g. dragging a file from a file manager), and for window interactive move when targeting the tiling layout.

The options are:

- `trigger-width`: size of the area near the monitor edge that will trigger the scrolling, in logical pixels.
- `delay-ms`: delay in milliseconds before the scrolling starts.
Avoids unwanted scrolling when dragging things across monitors.
- `max-speed`: maximum scrolling speed in logical pixels per second.
The scrolling speed increases linearly as you move your mouse cursor from `trigger-width` to the very edge of the monitor.

```kdl
gestures {
    // Increase the trigger area and maximum speed.
    dnd-edge-view-scroll {
        trigger-width 100
        max-speed 3000
    }
}
```

### `dnd-edge-workspace-switch`

<sup>Since: 25.05</sup>

Scroll the workspaces up/down when moving the mouse cursor against a monitor edge during drag-and-drop (DnD) while in the overview.
Also works on a touchscreen.

The options are:

- `trigger-height`: size of the area near the monitor edge that will trigger the scrolling, in logical pixels.
- `delay-ms`: delay in milliseconds before the scrolling starts.
Avoids unwanted scrolling when dragging things across monitors.
- `max-speed`: maximum scrolling speed; 1500 corresponds to one screen height per second.
The scrolling speed increases linearly as you move your mouse cursor from `trigger-width` to the very edge of the monitor.

```kdl
gestures {
    // Increase the trigger area and maximum speed.
    dnd-edge-workspace-switch {
        trigger-height 100
        max-speed 3000
    }
}
```

### `hot-corners`

<sup>Since: 25.05</sup>

Put your mouse at the very top-left corner of a monitor to toggle the overview.
Also works during drag-and-dropping something.

`off` disables the hot corners.

```kdl
// Disable the hot corners.
gestures {
    hot-corners {
        off
    }
}
```

<sup>Since: 25.11</sup> You can choose specific hot corners by name: `top-left`, `top-right`, `bottom-left`, `bottom-right`.
If no corners are explicitly set, the top-left corner will be active by default.

```kdl
// Enable the top-right and bottom-right hot corners.
gestures {
    hot-corners {
        top-right
        bottom-right
    }
}
```

You can also customize hot corners per-output [in the output config](./Configuration:-Outputs.md#hot-corners).

Two independent hot-corner blocks can invoke Exposé instead of the overview:

```kdl
gestures {
    // Show the windows from each monitor on that monitor.
    hot-corners-expose {
        top-right
    }

    // Gather windows from every monitor on the monitor containing the pointer.
    // An empty block uses the top-left corner.
    hot-corners-expose-all-outputs {}
}
```

These blocks accept the same `off`, `top-left`, `top-right`, `bottom-left`, and `bottom-right`
settings as `hot-corners`. They are disabled when omitted. If the same corner is assigned to
multiple actions, `hot-corners-expose-all-outputs` takes priority, followed by
`hot-corners-expose`, then `hot-corners`.
