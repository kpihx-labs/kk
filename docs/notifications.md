# Local Message Notifications

Message alerts are an opt-in profile preference owned by the backend. They are
off by default. The browser's notification permission is a separate requirement;
changing settings through the CLI never grants it.

```sh
kk --profile-name work profile notifications --enable
kk --profile-name work profile notifications --disable
```

In the running UI, open Settings, choose **Allow notifications in this browser**,
then save the content-free message notification preference. A denied/unsupported
permission is explained without silently enabling it. A pending prompt is visible;
closing or replacing the dialog cannot apply a late grant to another dialog.
The browser may require a new grant when the local UI origin/port changes.

## Privacy And Lifetime

- Only new incoming, nonsensitive messages can alert. No alerts for initial state,
  history pages, outgoing messages, old polling snapshots or enabling the feature.
- Selected-room messages already visible in a focused page do not alert.
- OS title/body are fixed generic text, without message bodies, names, room titles,
  previews, references or private API credentials. Sensitive messages never alert.
- The client retains one alert and one timer, replaces the old alert, and fences
  late click/timer callbacks. Removed rooms, opt-out, permission loss and auth
  retirement close/invalidate active alerts. A platform failure is not retried
  until an explicit new permission action.
- Alerts require the foreground UI service and its browser page to remain open;
  this does not install a hidden background service or create push subscriptions.
- Alerts are not delivery/read receipts and can appear on the OS lock screen.

The settings writer applies only explicitly changed fields under its existing
revision/lock checks. Stale dialogs reload both notification and name-sharing
consent before a subsequent save; an unrelated edit cannot re-enable an opt-out.
Missing legacy preference means off; false is omitted on serialization.

## Executed Gates

The focused checks cover backend defaults/legacy metadata, locked updates and
conflicts, real CLI enable/disable/profile isolation, and authenticated HTTP
settings. Browser tests cover explicit permissions, sensitive/private-field
exclusion, no backfill/replay, bounded lifetime and stale callbacks/dialogs.

Real Edge (`default`) exercised the unchanged Alex/Morgan profiles in foreground
panes `%122`/`%125`, on `127.0.0.1:45466`/`:45468`: default-off display, explicit
enable refused while permission remained default, no backend opt-in after refusal,
successful ordinary messages in both directions, then authenticated shutdown.
The profiles retain their identities; the QA pages are blank afterwards.

Actual OS notification acceptance/display remains **unqualified**: the native
permission prompt did not resolve, and desktop activation returned AT-SPI focus
refused. No permission API override, browser-policy change, blind click or capture
of the operator's desktop was used. This is a bounded open gate, not completion
of the notification feature or the full product; independent work continues.
