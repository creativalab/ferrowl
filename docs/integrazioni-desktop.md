# Ferrowl programmabile — piano per il desktop

Stato: proposta, 9 novembre 2026. Da leggere e realizzare sul **PC desktop**;
in parallelo, su un altro computer, si fa la parte mobile (iOS/Android).
Questo documento dice cosa fa il desktop e cosa deve restare comune.

## Obiettivo

Ferrowl diventa il punto da cui **altri programmi, script e assistenti AI
leggono, cercano e usano la posta, i contatti e il calendario**:
- in locale, sul dispositivo dell'utente;
- con il suo permesso;
- con qualsiasi provider IMAP.

Gli altri client non lo fanno:
- Gmail e Outlook hanno API solo cloud e solo per sé;
- Spark e Superhuman tengono l'AI sui loro server;
- Thunderbird ha plugin interni, non un'interfaccia per altri programmi.

Pubblico: **tutti**, sia utenti normali sia professionisti e aziende. Per
questo ogni funzione deve avere anche un'interfaccia comprensibile (permessi,
registro), non solo la riga di comando.

Decisioni prese con l'utente:
- si parte **dal mobile** (App Intents/Comandi Rapidi/Siri, intent Android);
- il desktop segue con questo documento;
- divisione gratis/Pro approvata, dettagli più sotto.

## Architettura comune: Ferrowl Bridge (in `core/`)

> **Aggiornamento 9 novembre 2026, branch `feat/mobile-intents`.** Il
> modulo `core/src/bridge.rs` esiste già e contiene:
> - link `ferrowl://open|inbox|compose` e `mailto:`, con `parse_link` e
>   `open_link`;
> - `Capability` con `needs_pro` e `needs_confirmation`;
> - `search` per qualsiasi account, sulla cache.
>
> L'FFI lo espone come `parse_link`, `message_link`,
> `capability_needs_pro`, `capability_needs_confirmation` ed
> `Engine::search_account`. Il desktop deve partire da qui e aggiungere le
> azioni restanti nello stesso modulo.
>
> Testi: chiavi `intent_*` e `shortcut_*` in `core/locales`.

Un solo insieme di azioni con un formato stabile, nel motore Rust: lo usano
il desktop, l'FFI di iOS/Android e tutte le "porte" verso l'esterno.

- **Modulo:** `core/src/bridge.rs`, che nasce dal lavoro mobile. Prima di
  iniziare il desktop, fare `git pull` e controllare se esiste già.
  - Se esiste, usarlo ed estenderlo, senza duplicare.
  - Se non esiste ancora, crearlo con questa forma e dirlo nella PR, così il
    mobile si allinea.
- **Azioni:** ognuna è una funzione `async` che riceve parametri serializzabili
  (serde) e restituisce JSON. I nomi sono stabili: sono l'API pubblica.

| Azione | Permesso | Note |
|---|---|---|
| `accounts.list` | `read` | email e nome, mai le credenziali |
| `mail.search` | `read` | testo con la sintassi di `query.rs` (`from:` `has:attachment` …), cartella opzionale, limite |
| `mail.unread` | `read` | non lette per account/cartella, ultime N |
| `mail.get` | `read` | intestazioni, testo semplice, elenco allegati; HTML solo se chiesto |
| `mail.attachment` | `read` | salva un allegato in un percorso dato |
| `mail.draft` | `write` | crea una bozza, non invia |
| `mail.send` | `send` | invio; vedi conferme |
| `mail.reply` / `mail.forward` | `send` | usano `reply_draft`/`forward_draft` |
| `mail.move` / `mail.archive` / `mail.trash` / `mail.mark` / `mail.label` | `write` | |
| `contacts.find` | `contacts` | da CardDAV e dalla cache |
| `calendar.agenda` / `calendar.create` | `calendar` | da CalDAV (`dav.rs`, `events.rs`) |
| `events.subscribe` | `read` | mail nuova, mail letta, invito ricevuto |

- **Identificativi stabili:** un messaggio è `account/cartella/uid` (più
  `uidvalidity` per accorgersi dei cambi) e ha anche il `Message-ID`, così un
  programma può ritrovarlo dopo.
- **Permessi** (`read`, `write`, `send`, `contacts`, `calendar`) per ogni
  client autorizzato, salvati nella config di Ferrowl.
- **Registro:** ogni chiamata viene scritta con data, client, azione ed esito,
  in SQLite accanto alla cache.
- **Attenzione al lock:** il `Backend` ha un `Mutex` tenuto per tutta la
  durata dei comandi IMAP. Le chiamate del Bridge che lavorano sul server
  devono usare una connessione propria (come `Backend::peek`) o una coda, e
  non bloccare mai l'interfaccia.
  - Le letture partono dalla cache SQLite/FTS quando basta.

## Le porte del desktop

### 1. Server MCP — priorità 1

È la cosa più nuova e dimostrabile: "usa la tua posta con Claude, in locale".

- `ferrowl mcp` avvia un server MCP su stdio, la forma che usano Claude
  Desktop, Claude Code e gli altri assistenti.
  - Ogni azione del Bridge diventa un tool MCP (`search_mail`, `get_message`,
    `send_mail`…), con descrizioni chiare.
  - Ci sono anche i prompt pronti: "riassumi le non lette", "trova le fatture
    del mese".
- In Impostazioni → Integrazioni, il pulsante **"Collega a Claude Desktop"**
  scrive la voce nel file di configurazione di Claude Desktop, dopo aver
  chiesto conferma. Mostra anche lo snippet da copiare per gli altri client.
- **Come parla con l'app:** se Ferrowl è aperto, il processo `ferrowl mcp`
  passa dall'istanza principale. Si estende `single_instance.rs`: oggi ascolta
  su loopback con un token e riceve solo "mostra la finestra", e diventerà un
  canale di richieste JSON.
  - Se l'app è chiusa, `ferrowl mcp` lavora da solo con il core: stessa
    config, cache e credenziali dal portachiavi.
- **Sicurezza contro la prompt injection:** il testo delle mail è dato, non
  istruzione.
  - Le azioni `send` e quelle distruttive chiedono conferma nell'app (una
    notifica o un dialogo "Claude vuole inviare questa mail a X — Consenti /
    Rifiuta"), a meno che l'utente non abbia scelto "consenti sempre" per quel
    client.
  - Nei risultati verso l'AI, il contenuto delle mail è marcato come contenuto
    esterno.

### 2. Riga di comando — priorità 1, stessa base dell'MCP

- `ferrowl search "from:anna has:attachment" --json`
- `ferrowl unread --account x@y --json`
- `ferrowl get <id> [--html]`
- `ferrowl send --to … --subject … --body-file … --attach …`
- `ferrowl attach <id> --save-dir …`
- `ferrowl watch --json`: un evento per riga, comodo nelle pipe.

Il parsing degli argomenti va in `main.rs` prima di avviare iced: senza
sottocomando parte l'interfaccia come oggi. Le uscite sono in JSON stabile e i
codici di uscita sono chiari.

### 3. Regole automatiche — priorità 2

- "Quando arriva una mail che… allora…".
  - Condizioni: mittente, oggetto, corpo, allegati, account, etichetta, con la
    stessa sintassi della ricerca.
  - Azioni: sposta, etichetta, inoltra, salva allegati in una cartella, esegui
    uno script (con il messaggio in JSON su stdin), chiama un webhook (POST
    JSON).
- Il segnale c'è già: l'IDLE di `push.rs`.
- Le regole sono salvate nel formato comune del Bridge, così in futuro potranno
  girare anche su mobile, quando l'app è aperta o con la push.
- Si estendono le regole mittente già esistenti (`set_sender_rule`), senza
  sostituirle.
- L'editor delle regole è nelle Impostazioni, con "prova la regola sulle ultime
  50 mail" prima di attivarla.

### 4. API locale HTTP — priorità 3

- `http://127.0.0.1:<porta>` con un token per ogni client, generato nelle
  Impostazioni. Serve a gestionali, CRM, n8n o Node-RED in locale.
- Le stesse azioni del Bridge sono esposte come REST/JSON: `GET /mail/search?q=…`,
  `POST /mail/send`, eccetera.
- `GET /events` è uno stream SSE.
- Spenta di default, ascolta solo su loopback e chiede il token sempre.

### 5. Link `ferrowl://` — piccolo, da fare subito

- `ferrowl://open?id=…`, `ferrowl://compose?to=…&subject=…`, `mailto:`.
- Registrazione come gestore: `.desktop` su Linux (c'è `desktop_entry.rs`),
  `Info.plist` su macOS, registro su Windows.
- Gli stessi link valgono anche su iOS/Android, così un link creato da un
  programma apre la mail ovunque.

## Interfaccia: Impostazioni → Integrazioni

- Elenco dei programmi collegati, ciascuno con nome, permessi (spunte),
  "chiedi conferma prima di inviare", ultimo uso e pulsante "Scollega".
- Interruttori "Server MCP", "API locale" e "Regole", con stato e istruzioni.
- Registro attività filtrabile.
- Tutti i testi in `locales/*.toml`, nelle 10 lingue.

## Gratis e Pro (approvato)

| Gratis | Pro |
|---|---|
| Riga di comando in lettura | Invio e modifica da programmi esterni (CLI, MCP, API) |
| MCP in sola lettura | Regole automatiche con script/webhook |
| Link `ferrowl://` | API locale HTTP |
| | Integrazioni mobile avanzate |

Il controllo Pro sta nel Bridge, non nelle singole porte, così vale ovunque
allo stesso modo. Vedi `docs/monetizzazione.md`.

## Ordine di lavoro proposto (desktop)

1. **Bridge in `core/`:**
   - azioni di lettura;
   - permessi;
   - registro;
   - test unitari sul formato JSON.

   Va coordinato con il mobile: se il mobile l'ha già iniziato, si parte da lì.
2. **CLI:** sottocomandi in `main.rs`, uscita JSON.
3. **MCP:** `ferrowl mcp` su stdio, canale con l'istanza aperta, "Collega a
   Claude Desktop" e conferme di invio.
4. **Impostazioni → Integrazioni:** permessi, registro, traduzioni.
5. Link `ferrowl://`.
6. Regole automatiche.
7. API locale HTTP.

Ogni passo va in una PR separata, con CI verde e merge.

## Regole del progetto da rispettare

- Codice e commenti in inglese, messaggi all'utente in italiano.
- Le modifiche al core devono compilare anche per iOS/Android. Controllare
  l'FFI in `ffi/`: se un tipo del Bridge serve al mobile, esporlo lì.
- Niente dati delle mail verso servizi esterni. Il server push resta "cieco".
- Nessuna credenziale nel registro o nelle risposte JSON.
