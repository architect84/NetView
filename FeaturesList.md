
1. [x] Filter needs to filter any column. Completed: `/` searches local/remote endpoints, state, PID, and process (case-insensitive).
2. [x] Columns need to be able to sort. Completed: `1`–`5` sort Local, Remote, State, PID, and Process; repeat the active key to reverse direction. Headers show `↑`/`↓`.
3. The user needs to be able to use the arrow keys to highlight a connection in any column and the press a button to filter our everything else.
4. Need to add GeoIP lookup using MaxMind.
    - Need a json config for enabling/disabling the download of the database and the api key for the payed version.
    - In the json config file we need an option to turned on/off GeoIP
    - Add a colomn for ASN, Country and City.
    - Add a way to change the settings in the config file from the interface from a menu.
5. Need to be able to resolve remote ip addresses. This feature needs to be able to be turned on/off.
6. [x] Needs a column to show inbound/outbound connections. Completed: Direction in TUI and snapshots, searchable and sortable with `6`; shows INBOUND/OUTBOUND from observed handshake states, `?` for inferred directions, LISTENING for listeners, and UNKNOWN when undetermined.
7. Needs a column for highlighting the connections to keep at the top of the list to watch/monitor them.
