/** Starts the Manager once per process; `filesDir` holds data and bundled resources. */
export const start: (filesDir: string) => void;
/** Ability moved to the foreground (true) or background. */
export const foreground: (active: boolean) => void;
/** System color mode and the vp-to-px ratio of the default display. */
export const configure: (dark: boolean, scale: number) => void;
/** Forwards an OH_NativeXComponent_KeyCode press (true) or release. */
export const pressKey: (code: number, pressed: boolean) => void;
/** Receives Manager requests: error, exit, restart, openPath, pointerVisible and pickers. */
export const setEventListener: (listener: (kind: string, payload: string) => void) => void;
/** Completes picker `id` with a filesystem path, or an error ("cancelled" when dismissed). */
export const completePicker: (id: number, path: string, error: string) => void;
