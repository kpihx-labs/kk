# Installation Evidence Boundary

The active refactor needs fresh exact-artifact checks; a working installer or
passing parser fixture is not a qualified release. Current state: [progress](progress.md).

```text
reviewed selectors -> authenticate archive/helper/notices -> install exact bytes
                                                               |
                                               update -> rollback -> compare bytes
```

| Gate | Required evidence |
| --- | --- |
| Origin | Expected repository, workflow, tag and full source commit |
| Content | Manifest/member hashes and installed executable match the authenticated artifact |
| Managed state | Fresh prefix, update and rollback; unrelated installation/profiles unchanged |
| Offline | Separately trusted roots/bundles and recorded isolation; preparation alone is not execution |

This target remains for the unchanged [lab README](../lab/README.md).
Earlier stable/candidate bindings remain in [Git](https://github.com/kpihx-labs/kk/blob/44f1a88c976eb44c7886df9233c980ce96ab6383/docs/install-qualification.md).
Instructions: [installers](installers.md), [offline/USB](offline-install.md).
