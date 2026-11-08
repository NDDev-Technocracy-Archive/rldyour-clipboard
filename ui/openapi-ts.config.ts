import { defineConfig } from "@hey-api/openapi-ts";
export default defineConfig({
  input: "../contracts/local-ipc.openapi.json",
  output: "generated",
  plugins: ["@hey-api/typescript"],
});
