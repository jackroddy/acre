# acre

> **<u>a</u>nimation <u>c</u>reation and <u>r</u>endering <u>e</u>ngine**

## about
`acre` reads a drawing written in its own small language and renders it to an
image. The language says what is on the canvas. Animation comes later.
It is early. The language covers boxes, ellipses, lines, arrows and text, and its
syntax will change.
The tool is purpose-built for my own use cases, and it is primarily developed using Claude Code.

## usage

```sh
acre check drawing.acre
acre render drawing.acre --out drawing.png
```

## crates

| crate | what it holds |
|---|---|
| `acre` | the library: language, scene model and renderer |
| `acre-cli` | the `acre` command |
| `acre-gui` | the editor, not written yet |
