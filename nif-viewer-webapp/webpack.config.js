const CopyWebpackPlugin = require("copy-webpack-plugin");
const path = require("path");

module.exports = {
  entry: "./bootstrap.js",
  output: {
    path: path.resolve(__dirname, "dist"),
    filename: "bootstrap.js",
  },
  mode: "development",
  experiments: {
    asyncWebAssembly: true,
  },
  plugins: [
    new CopyWebpackPlugin({
      patterns: [
        "index.html",
        {
          from: "../resources",
          to: "resources",
          noErrorOnMissing: true,
        },
        {
          // Staged from the renderer package: manifest.json + WGSL.
          from: "../scene-renderer/shaders",
          to: "shaders",
        },
      ],
    }),
  ],
};
