export const GECKO_ID: string
export const REPO: string
export const UPDATE_URL: string
export function updatesJson(
  version: string,
  tag: string,
): { addons: Record<string, { updates: { version: string; update_link: string }[] }> }
