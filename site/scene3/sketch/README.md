# Scene 3 sketch studies

The selected `living-garden` study is one repeated species: a small aperture form with a narrow body and two opening wings. Its main construction follows Faithful port 04's positive-e gate, where `k` and `e` choose the surviving points, `d` expands the distance field, and `c`/`o` fold that field into the dotted wing and body. Each individual holds one fixed `m` family and subtracts the source's `d=4.6` baseline before placement. Port 01's `d-t` orbit contributes only slow coherent drift; port 02's `d*8-t*3` pulse contributes only a low-amplitude breathing change to filament alpha. Those borrowed traits modulate the same aperture rather than adding separate forms.

The source examples in `moss-landing-lab/scene3/sketch/processing-examples.md` state the original point equations and timing. The species interpretation and the choice to borrow port 01/02 as motion and alpha controls are design inference. The native renderer keeps 10 bounded individuals in two deliberate rows plus side forms, uses fixed `m` offsets to vary pose without changing anatomy, and samples a cached 1,800-point field; `count`, `samples`, `.075` drift rate, `.48` filament phase, `m = organism * .43`, and the `.28 + .20 * pulse` alpha range are the tunable parameters.

| Study / parameter | What it controls | Design lesson |
|---|---|---|
| 01: `q`, `d-t`, vertical multiplier `35` | Sweeps a narrow point field into a long twisting veil; changes its phase over time | Change the envelope before adding more particles; a coherent silhouette reads as one organism. |
| 02: `m=i%4*5`, `89*sin(c)`, `79*sin(2*c)` | Four phase families following a two-frequency orbit | A few related bodies can share one equation without moving in lockstep. |
| 02: `d*8-t*3` | Radial detail and its pulse speed | Let surface detail move faster than the whole body. |
| 04: `m=i%19` | Nineteen interleaved phase families | One source image already contains many creatures; repeating the whole field makes clutter. |
| 04: `hypot(k,e)^3/999` and `3^o` | Concentrates the interior and stretches the outer filaments; varies width | Small changes strongly alter anatomy. Preserve these while changing placement and timing first. |
| 04: `e>0` and `e^o` | Keeps one side of the field and produces the fine elongated tips | Adding a constant to `e` removes delicate structure; retain the original power and reject non-finite points. |

These are readings of the equations, not claims about the original author's intentions. For the next variation, first adjust population spacing and body scale, then the slow travel rate, and only then the fold exponent. One promising new direction is a delayed pulse travelling from the body into the tips, so each creature appears to propel itself rather than merely rotate. That is a proposal, not an implemented behavior.
