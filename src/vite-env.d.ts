// Vite resolves asset imports to a URL string at build time.
declare module "*.png" {
  const url: string;
  export default url;
}
