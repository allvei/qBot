# The trends.tf API

trends.tf provides a HTTPS-based API to access some of its information. API calls are made by sending requests to endpoints, which return JSON-encoded responses.

## Requests

Endpoints are specified as an HTTP method and a URL. All endpoints support the `OPTIONS` method. Additionally, endpoints supporting the `GET` method also support the `HEAD` method. Endpoint URLs implicitly start with `https://trends.tf/api`. For example, an endpoint such as

```
GET /foo
```

would have a full URL of `https://trends.tf/api/foo`. The `If-None-Match` and `If-Modified-Since` headers are supported, and may be used to re-use cached responses.

Information may be passed to requests via arguments or parameters. Arguments are part of the path component of the URL. For example, when accessing the (imaginary) endpoint `/v1/foo/<bar>/baz` at `/v1/foo/29/baz`, the argument `bar` is set to `29`.

Parameters are passed via the query component of the URL. Parameters consist of key/value pairs where `&` separates parameters from each other, and `=` separates keys from values. Parameters are generally referred to by their keys. Values must be URL-encoded. Continuing the above example, if the requested URL was instead `/v1/foo/29/baz?qux=33&frob=forty%20two`, the parameter `qux` would be set to `33`, and the parameter `frob` would be set to `forty two`. Passing an empty value to a parameter is equivalent to omitting the parameter.

## Responses

Responses are JSON-encoded objects. Object members are specified like:

```
name : type
```

where `name` is the name of the member, and `type` is one of:

| Type | Description |
|------|-------------|
| `bool` | Either `true` or `false` |
| `number` | An IEEE 764 double-precision literal |
| `string` | A UTF-8-encoded string |
| `object` | A nested object |
| `option(type)` | Either `type` or `null` |
| `array(type)` | An array of one or more types |

All documented object members will always be present. Absence of a value will never be represented by the absence of the member itself. Values of non-option types will never be `null`.

Responses use HTTP status codes to indicate status. Clients must support HTTP 3XX (redirect) status responses. Clients should parse the status code before interpreting the response body, as an error status may change the format of the response. 5XX (server error) responses may result in content types which are not `application/json`.

All responses include the `ETag` header. Additionally, some endpoints include the `Last-Modified` header. If the results are paged, the `Last-Modified` header reflects the last-modified time of the whole set of results. The `Last-Modified` header may be more pessimistic than necessary, returning times later than when the result was last modified.

### Error Responses

Error responses may include the following field:

```
error : object
  code : number           # The HTTP status code
  description : string    # An informational description of the error
  name : string           # The name of the error
```

## Rate Limits

To ensure that the site remains available to all users, requests are currently limited to **10 per minute** across the site. A small number of requests in excess of this rate will be served as usual. Further requests in excess of this rate will result in `429` responses. Repeatedly exceeding these rate limits will result in your IP address being banned. This ban will automatically lift after one hour. Static content is not rate-limited.

## Parameters

This section documents parameters with shared semantics across multiple endpoints.

### `date_from`
Filter the results to those occurring on or after midnight on the specified RFC 3339 full date. Midnight is determined based on the value of the `timezone` parameter.

**Example:** Supplying `date_from=2022-03-04` and `timezone=America/New_York` to the `/v1/logs` endpoint would filter results to logs occurring on or after `2022-03-04 05:00:00Z`.

### `date_to`
Filter the results to those occurring on or before midnight on the specified RFC 3339 full date. Midnight is determined based on the value of the `timezone` parameter.

**Example:** Supplying `date_to=2022-03-04` and `timezone=America/New_York` to the `/v1/logs` endpoint would filter results to logs occurring on or before `2022-03-04 05:00:00Z`.

### `format`
Filter the results to logs with matching formats.

**Example:** Supplying `format=sixes` to the `/v1/logs` endpoint would filter results to 6v6 logs.

### `league`
Filter the results to those associated with a league.

**Example:** Supplying `league=rgl` to the `/v1/logs` endpoint would filter results to RGL matches.

### `limit`
Return at most this many results. Unless otherwise stated, this parameter defaults to `100`. Specifying a value greater than the default results in the default.

**Example:** Supplying `limit=10` to the `/v1/logs` endpoint would return at most 10 matches.

### `map`
Filter the results to those including the value of this parameter (case-insensitive) in the map name.

**Example:** Supplying `map=cp_` to the `/v1/logs` endpoint would filter results to control-point logs.

### `sort`
This parameter specifies the key to use when sorting the output. The valid keys vary between endpoints, although they generally denote similar meanings. Results are always sorted.

### `sort_dir`
This parameter specifies the sort direction as either `asc` (ascending) or `desc` (descending).

- **Ascending** sorts satisfy the relation that one result comes before another if and only if the value of the first result's sort key is less than or equal to the other result's sort key.
- **Descending** sorts satisfy the relation that one result comes before another if and only if the value of the first result's sort key is greater than or equal to the other result's sort key.

### `steamid64`
Filter the results to those including this steamid64. This parameter may be specified up to **five times** to filter by additional players.

**Example:** Supplying `steamid64=76561197970669109` and `steamid64=76561198053621664` to the `/v1/logs` endpoint would filter results to those where b4nny and habib both played.

### `time_from`
Filter the results to those occurring at or after the specified UNIX time.

**Example:** Supplying `time_from=1646334000` to the `/v1/logs` endpoint would filter results to logs occurring on or after `2022-03-04 05:00:00Z`.

> **Note:** If either `date_from` or `date_to` is present, they override this parameter.

### `time_to`
Filter the results to those occurring at or before the specified UNIX time.

**Example:** Supplying `time_to=1646334000` to the `/v1/logs` endpoint would filter results to logs occurring on or before `2022-03-04 05:00:00Z`.

> **Note:** If either `date_from` or `date_to` is present, they override this parameter.

### `timezone`
This parameter supplies the time zone for use with the `date_from` and `date_to` parameters. It must be the name of a time zone as specified in the IANA time zone database. If this parameter is invalid or unspecified, the time zone defaults to UTC.

### `title`
Filter the results to those including the value of this parameter (case-insensitive) in the title of the log.

**Example:** Supplying `title=center` to the `/v1/logs` endpoint would filter results to TF2Center lobbies.

### `updated_since`
Filter the results to those updated after the specified UNIX time.

**Example:** Supplying `updated_since=1646334000` to the `/v1/logs` endpoint would filter results to logs updated after `2022-03-04 05:00:00Z`.

## Members

This section documents members common across multiple responses.

### `demoid`
A unique number identifying a demo. This is assigned by demos.tf.

### `format`
A game format as one of the following values:

| Value | Description |
|-------|-------------|
| `ultiduo` | 2v2 |
| `fours` | 4v4 |
| `sixes` | 6v6 |
| `prolander` | 7v7 |
| `highlander` | 9v9 |
| `other` | Other formats |

> **Note:** Format detection is based on player count and playtime. This may cause some formats to be detected as others if they share the same nominal number of players.

### `league`
A competitive league as one of the following values:

| Value | Description |
|-------|-------------|
| `etf2l` | European Team Fortress 2 League |
| `rgl` | Recharge Gaming League |

### `logid`
A unique number identifying a log. This is assigned by logs.tf.

### `matchid`
A unique number identifying a match played as part of a league. `matchid`s are only valid in the context of a particular league; different leagues may reuse the same `matchid`. `matchid`s are assigned by their respective leagues.

### `next_page`
A relative URL to the next page of results. If the number of returned results is equal to the limit, then this field may be non-null even if the next page is empty.

### `steamid64`
A unique number identifying a player. This is assigned by Valve.

### `teamid`
A unique number identifying a team in a league across multiple competitions. `teamid`s are only valid in the context of a particular league; different leagues may reuse the same `teamid`. `teamid`s are generally assigned by their respective leagues. The `teamid`s used for RGL teams correspond to the minimum RGL `teamId` used by a team across multiple seasons.

### `time`
Unix time. That is, the number of non-leap seconds since `1970-01-01 00:00:00Z`.

## Endpoints

### GET /v1/logs

The `/v1/logs` endpoint provides a list of log summaries similar to what is presented on the logs page.

#### Parameters

The `/v1/logs` endpoint supports the following common parameters:

- `date_from`
- `date_to`
- `format`
- `league`
- `limit`
- `map`
- `steamid64`
- `sort`, with valid values being:
  - `logid` - Sort by logid
  - `date` - Sort by time
  - `duration` - Sort by duration
  - `updated` - Sort by updated
- `sort_dir` (default: `desc`)
- `time_from`
- `time_to`
- `timezone`
- `title`

The `/v1/logs` endpoint also supports the following additional parameters:

#### `view` (default: `basic`)
Return additional members for each log, as described below. Valid values are:

| Value | Description |
|-------|-------------|
| `basic` | The default with no additional members |
| `players` | Include additional members for players in the log |

#### `include_dupes` (default: `yes`)
Include duplicate logs in the output. If `yes`, then `duplicate_of` may be non-null. If `no`, then such logs will not be included with the results.

#### Response

```
logs : array(object)
```

Each element corresponds to a log:

| Member | Type | Description |
|--------|------|-------------|
| `demoid` | `option(number)` | The linked demoid, if any |
| `duplicate_of` | `option(array(number))` | An array of logids that this log is a duplicate of (which may themselves be duplicates), if any. Duplicate logs are typically combined or otherwise duplicated versions of other logs. They should not be used for statistical purposes. |
| `duration` | `number` | The duration of the log, in seconds |
| `format` | `option(string)` | The format of the log |
| `league` | `option(string)` | The league of the linked match, if any |
| `logid` | `number` | The logid of the log |
| `map` | `string` | The (user-supplied) map this log was played on |
| `matchid` | `option(number)` | The matchid of the linked match, if any |
| `time` | `number` | The upload time of the log |
| `title` | `string` | The (user-supplied) title for the log |
| `updated` | `number` | The time this object was last modified |

Additionally, if the parameter `view` is `players`, the following members are included:

```
blue, red : object
```

Information about the blue and red teams:

| Member | Type | Description |
|--------|------|-------------|
| `players` | `array(string)` | An array of steamid64s, one for each player on the team |
| `rgl_teamid` | `option(number)` | The RGL teamId of this team. This ID is only valid for use with the RGL API, and cannot be used elsewhere on trends.tf. This member is only present when `league` is `rgl`. |
| `teamid` | `option(number)` | The teamid of this team, if there is a linked match |
| `score` | `number` | The points scored by this team. This is the raw number as reported on the scoreboard, and does not reflect rounds played. For instance, if there is a stalemate due to the round timer this will not be reflected in either score, and if there is a stalemate due to the time limit this will be an extra point for the winning team. |

```
next_page : option(string)
```

The next_page of logs, if any.
