// API do navegador: `browser` no Firefox, `chrome` no Chromium. As duas devolvem
// Promises no MV3, então os tipos do Chromium servem para as duas.

declare const browser: typeof chrome | undefined

export const api: typeof chrome = typeof browser !== 'undefined' ? browser : chrome

/** `runtime.getBrowserInfo` só existe no Firefox. */
export const isFirefox = 'getBrowserInfo' in api.runtime

export const HOST = 'io.github.langendolff.tidyflow'
