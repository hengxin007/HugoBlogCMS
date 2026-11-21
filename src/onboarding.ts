export type Route = 'create' | 'import'
export type Site = { title: string; remote: string; branch: string; postsDir: string; website: string; hostedImages: boolean }
export type SetupInput = {
  title: string; remote: string; branch: string; rootDir: string; postsDir: string; website: string
  pagesProject: string; pagesUrl: string; account: string; hugoPath: string; proxy: string; confirmed: boolean
}
export type Wizard = {
  id: string; route: Route; step: number; input: SetupInput; firstCommit: string; hugoVersion: string
  theme: string; candidates: string[]; articles: number; unsupported: number; checked: Site | null; deployment: string
}
export type Startup = { site: Site | null; wizard: Wizard | null; showWelcome: boolean; pagesBuild: string }
