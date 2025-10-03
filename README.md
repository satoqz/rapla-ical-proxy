# rapla-ical-proxy

This tool proxies requests to [DHBW](https://www.dhbw.de/english/home)'s HTML
[class schedule site](https://rapla.dhbw.de) into [ICS](https://icalendar.org)
calendars on the fly. This lets you import your class schedule into "real"
calendar software such such as Outlook, Google Calendar, etc. and keep
automatically receiving the latest schedule.

> [!TIP]
> If you study at DHBW and want to view your class schedule together with your
> work schedule in one and the same calendar app, **this is what you're looking
> for**.

## Guide

Getting started is easy and requires zero setup if you use the official instance
at [rapla.satoqz.net](https://rapla.satoqz.net).

1. Get your Rapla link ready. This should be a decently long URL of the
   following shape:

  ```yaml
  https://rapla.dhbw.de/rapla/...
  ```

2. Replace the domain name `rapla.dhbw.de` with `rapla.satoqz.net` (Or the
   hostname of another instance, see [self-hosting](#self-hosting)!). **Keep all
   other URL components the same!**

  ```diff
  - https://rapla.dhbw.de/rapla/rest
  + https://rapla.satoqz.net/rapla/rest
  ```

3. Create a new calendar subscription in your calendar app. Paste in the
   modified URL. Done!

### Advanced Usage

By default, you will always receive any available events in within the `(now - 1
year, now + 1 year)` range.

If you'd like to avoid filling past calendar history with events beyond a
certain date, you can add the `cutoff_date` URL parameter:

```yaml
https://rapla.dhbw.de/rapla/calendar?other=parameters&cutoff_date=YYYY-MM-DD
```

This will shift the two-year range that is scanned by default to start at the
specified cutoff date.

## Self-hosting

The proxy is a simple single-binary webserver with no external dependencies.
You can deploy it on a VPS, serverless, or even on your local system directly in
front of your calendar software.

### Container Images

Distroless container images are tagged by commit hash and available for both
`linux/amd64` and `linux/arm64`.

```sh
# Pull the latest commit:
docker pull ghcr.io/satoqz/rapla-ical-proxy:latest

# Pull a specific commit by full hash:
docker pull ghcr.io/satoqz/rapla-ical-proxy:$GIT_HASH

# Or by short hash:
docker pull ghcr.io/satoqz/rapla-ical-proxy:$SHORT_GIT_HASH

# Run it:
docker run --rm -d -p 8080:8080 -e RAPLA_CACHE_MAX_SIZE=100 ghcr.io/satoqz/rapla-ical-proxy

# Make sure it works:
curl -I http://localhost:8080/rapla/calendar/...
````

### Environment Variables

The proxy respects the following environment variables:

| Environment            | Default          | Description                                  |
| ---------------------- | ---------------- | -------------------------------------------- |
| `RAPLA_ADDRESS`        | `127.0.0.1:8080` | Socket address to listen at                  |
| `RAPLA_CACHE_TTL`      | `3600` (1 hour)  | Time-to-live for cached calendars in seconds |
| `RAPLA_CACHE_MAX_SIZE` | `0`              | Maximum (estimated) cache size in Megabytes  |

> [!NOTE]
> Setting `RAPLA_CACHE_MAX_SIZE` to `0` (the default) effectively disables
> caching. For production usage, I recommend allocating at least a couple of
> megabytes to caching. This saves a lot of network traffic and CPU time both on
> the proxy host and the upstream Rapla server.
