// Engine-independent signal: selection failed but the previous presentation
// remains usable. The UI must keep the previous selection and preferences.
export class AudioSelectionRetainedError extends Error {}
