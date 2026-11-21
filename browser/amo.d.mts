export const GECKO_ID: string
export const REPO: string
export const APPROVAL_NOTES: string
export function metadata(licenseText?: string): {
  categories: { firefox: string[] }
  summary: Record<string, string>
  description: Record<string, string>
  homepage: { url: Record<string, string> }
  support_url: { url: Record<string, string> }
  version: {
    custom_license: { name: Record<string, string>; text: Record<string, string> }
    approval_notes: string
  }
}
