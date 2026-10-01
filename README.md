# acre

> **<u>a</u>nimation <u>c</u>reation and <u>r</u>endering <u>e</u>ngine**

## about
`acre` reads an animation written in its own small language and renders it to
frames. The language says what is on the canvas and how each thing changes over
time; acre works out every frame from that.
It is early: the CLI names its commands and nothing renders yet.
The tool is purpose-built for my own use cases, and it is primarily developed using Claude Code.

## usage

```sh
acre check scene.acre
acre render --out frames scene.acre
```
