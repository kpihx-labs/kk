# Lab Artifact And Contention Evidence

Current integration acceptance is pending: [progress](progress.md).
The [lab README](../lab/README.md) links here for interpreting candidate results.

```text
authenticated executable -> host digest == measured guest digests
                                       |
                 fresh decoded pixels + active bulk file + control progress
                                       |
                     verify recipient bytes -> restore QoS -> stop guests
```

| Observation | What it establishes |
| --- | --- |
| Report binds every executable digest | Results concern those actual bytes, not a newer rebuild |
| Decoded owned-display pixels during bulk | Real codec/network progress, not physical camera/microphone capture |
| ACK samples under shaping | Run observations, not a latency or sustained-load SLA |
| Null failure and empty cleanup errors | Inspect alongside actual guest/network/resource shutdown |

These checks do not establish signing, installation or native Windows/macOS support.
Earlier measured runs and failed attempts remain in [Git](https://github.com/kpihx-labs/kk/blob/44f1a88c976eb44c7886df9233c980ce96ab6383/docs/native-candidate-qualification.md).
Run safely and locate reports: [verification lab](lab.md).
