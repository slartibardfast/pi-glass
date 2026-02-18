const fs = require("fs");
const path = require("path");
const { webLightTheme } = require("@fluentui/tokens");

let css = ":root {\n";
for (const [key, value] of Object.entries(webLightTheme)) {
  css += `  --${key}: ${value};\n`;
}
css += "}\n";

fs.mkdirSync(path.join(__dirname, "dist"), { recursive: true });
fs.writeFileSync(path.join(__dirname, "dist", "tokens.css"), css);
console.log(`Wrote ${Object.keys(webLightTheme).length} tokens to dist/tokens.css`);
