# glassbox

Concolic path conditions for one instruction of one run. Needs a working libz3 at runtime. A GitHub release already ships it next to `seer`. A source build uses the Z3 you installed (see the repo README). Do not compile Z3 as part of this crate.

```
seer glassbox <RUN> --ix <N>
seer glassbox <RUN> --ix <N> --force
```
