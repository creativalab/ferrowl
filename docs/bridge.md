# Ferrowl Bridge: usare Ferrowl da altre app

Altre app, automazioni e assistenti possono leggere la posta, i contatti e
il calendario di Ferrowl e chiedergli di fare delle cose:
- sul telefono, con i permessi che dai tu;
- senza passare da server esterni;
- con qualsiasi casella di posta (IMAP, POP3, Gmail, Outlook…).

Leggere è gratis. Inviare o modificare la posta da altre app richiede
**Ferrowl Pro** e una **conferma** ogni volta, che sugli invii da Tasker
puoi spegnere.

## Link `ferrowl://` (iPhone, Android, desktop)

| Link | Cosa fa |
|---|---|
| `ferrowl://compose?to=a@b.it&cc=…&subject=…&body=…` | Apre una mail nuova già compilata (non la invia) |
| `mailto:a@b.it?subject=…` | Come sopra |
| `ferrowl://inbox` | Apre la Posta in arrivo (`?account=…` per un account preciso) |
| `ferrowl://open?account=a@b.it&uid=123` | Apre un messaggio. Le risposte delle azioni contengono questo link pronto |

I valori vanno codificati come negli indirizzi web (spazio = `%20`).

## iPhone: Comandi Rapidi e Siri

Nell'app Comandi Rapidi cerca "Ferrowl". Ogni azione restituisce degli
oggetti (mail, contatti, eventi) da passare alle azioni successive.

| Azione | Cosa fa | Note |
|---|---|---|
| Mail non lette | Le non lette dal server, di tutti gli account o di uno | Siri le legge ad alta voce |
| Cerca nella posta | Ricerca nella posta salvata sul telefono | Sintassi: `from:`, `subject:`, `has:attachment`, `is:unread` |
| Scrivi mail | Apre una mail compilata, anche con allegati | Non invia |
| Invia mail | Invia | Pro, chiede sempre conferma |
| Segna mail | Segna come lette o da leggere le mail ricevute da un'altra azione | Pro, chiede conferma |
| Archivia mail | Archivia le mail ricevute da un'altra azione | Pro, chiede conferma |
| Apri mail | Apre una mail in Ferrowl | |
| Dettagli mail | Testo, allegati e ciò che Ferrowl riconosce (numero di tracciamento, corriere e link, codice, importo e scadenza) | Non segna la mail come letta |
| La mia agenda | Gli impegni di uno o più giorni | |
| Trova contatto | Cerca nei contatti | |
| Nuovo evento | Crea un evento | Chiede conferma |

Frasi per Siri: "Mail non lette in Ferrowl", "Cerca nella posta di
Ferrowl", "Scrivi una mail con Ferrowl", "Invia una mail con Ferrowl",
"La mia agenda in Ferrowl", "Trova un contatto in Ferrowl", "Nuovo evento
in Ferrowl".

**Esempio: ogni mattina alle 8.** Automazione "Ora del giorno" → "Mail
non lette" → "Pronuncia testo" con il risultato.

**Esempio: spedizioni in un'app di tracking.** "Mail non lette" →
"Ripeti con ciascuno" → "Dettagli mail" → se "Numero di tracciamento" non
è vuoto, aprilo nell'app del corriere, oppure aggiungilo a una nota o a un
promemoria.

**Esempio: fatture al commercialista.** "Cerca nella posta" con
`from:fornitore@ has:attachment is:unread` → "Archivia mail"
→ "Scrivi mail" a commercialista@…, allegando i file.

## Android: condivisione, scorciatoie, Tasker

- **Condividi → Ferrowl**: testo o file (anche più di uno) diventano una mail nuova con gli allegati.
- **Tieni premuta l'icona**: "Scrivi mail" e "Mail non lette".
- **Widget**: le non lette di tutti gli account, con le ultime tre mail.

### Tasker (e MacroDroid, Automate…)

1. In Ferrowl apri **Impostazioni → Integrazioni → Collega un'app**, scegli l'app e copia la chiave.
2. Scegli i permessi:
   - **Leggere la posta**: non lette, ricerca, agenda, contatti, eventi di nuova posta;
   - **Inviare mail** (Pro);
   - **Modificare la posta** (Pro): archiviare, segnare letta o da leggere.
3. "Chiedimi conferma" è attivo di default: per ogni invio o modifica arriva una notifica con **Invia / Consenti** e **Non inviare**.

**Il modo più semplice: il plugin.**
- Azione → **Plugin → Ferrowl**: scegli cosa fare (non lette, cerca,
  dettagli, agenda, contatti, invia, archivia, segna letta o da leggere) e
  compila i campi, anche con variabili `%`. I risultati arrivano in
  `%fw_ok`, `%fw_status`, `%fw_error`, `%fw_count`, `%fw_text` e `%fw_json`.
- Profilo → Evento → **Plugin → Nuova mail in Ferrowl**: puoi filtrare per
  account, mittente e oggetto (vuoto = qualsiasi). Il profilo riceve
  `%fw_account`, `%fw_from`, `%fw_address`, `%fw_subject`, `%fw_count`,
  `%fw_link`, `%fw_text` e `%fw_json`.
- La prima volta che salvi un'azione o un evento del plugin, l'app che lo
  usa (Tasker…) viene collegata a Ferrowl con il permesso di lettura: lo
  vedi scritto prima di salvare.
- La chiave di collegamento è salvata nel task di Tasker: se esporti o
  condividi il task (anche su TaskerNet) togli prima l'azione Ferrowl, o
  dopo scollega e ricollega l'app in Impostazioni → Integrazioni per
  cambiare la chiave.

**In alternativa, gli intent.** In Tasker usa l'azione **Send Intent**:
- Package: `com.ferrowl.app`
- Target: Broadcast Receiver
- Extra: `token:<chiave>`, più gli extra dell'azione

| Azione (`com.ferrowl.app.action.…`) | Extra | Permesso |
|---|---|---|
| `UNREAD` | `account`, `limit` | Leggere |
| `SEARCH` | `query`, `account`, `limit` | Leggere |
| `AGENDA` | `day` (AAAA-MM-GG, oggi se manca), `days` | Leggere |
| `CONTACTS` | `query` | Leggere |
| `DETAILS` | `link`: un link `ferrowl://open` | Leggere. Nel `%json` trovi `text` (testo), `attachments` (allegati), e quando ci sono `shipment` (`carrier`, `number`, `url`), `code` e `payment` (`number`, `amount`, `due`). Non segna la mail come letta |
| `SEND` | `to`, `subject`, `body`, `account` | Inviare (Pro) |
| `ARCHIVE`, `MARK_READ`, `MARK_UNREAD` | `link`: uno o più link `ferrowl://open`, separati da a capo | Modificare (Pro) |

Puoi aggiungere `request:<id>` a qualsiasi azione: ti torna uguale nella
risposta, così sai a quale richiesta si riferisce.

**Risposte e eventi.** Ricevili con un profilo **Evento → Intent Received**:
- `com.ferrowl.app.event.RESULT` è la risposta a ogni azione. Contiene
  `%ok`, `%status` (`ok`, `waiting`, `sent`, `done`, `refused`), `%error`,
  `%count`, `%text` (un riassunto da leggere ad alta voce), `%json` e
  `%request`.
- `com.ferrowl.app.event.NEW_MAIL` arriva a ogni nuova mail. Contiene
  `%account`, `%count`, `%unread`, `%from`, `%subject`, `%text` e `%json`.

Le risposte vanno **solo** all'app collegata. Chi non ha una chiave valida
non riceve niente. In Impostazioni → Integrazioni trovi le ultime 50
operazioni.

**Esempio: dimmi chi ti scrive.** Profilo `NEW_MAIL` → "Dì" `%from: %subject`.

**Esempio: incolla il codice.** Profilo `NEW_MAIL` → `DETAILS` con il
link del primo messaggio nel `%json` → nel profilo `RESULT`, se c'è
`code`, "Imposta appunti" con il codice.

**Esempio: archivia le newsletter lette.** `SEARCH` con
`from:newsletter@ is:read` → nel profilo `RESULT`, leggi i link da `%json`
→ `ARCHIVE` con quei link.

## Regole automatiche (Impostazioni → Regole, Pro)

"Quando arriva una mail che… allora…".
- **Condizione**: si scrive come una ricerca, per esempio
  `from:negozio@ subject:ordine`.
- **Azioni**: segna come letta, aggiungi la stella, notifica silenziosa,
  archivia, sposta in una cartella.

Ogni regola vale per le mail nuove, una volta per mail, sull'account in
cui la crei.

## Sicurezza

- **Nessuna mail passa da altri server.** Il server push di Ferrowl riceve
  solo un segnale "c'è posta" e non legge i messaggi.
- **Il testo di una mail non può comandare niente.** Le azioni che inviano
  o modificano chiedono sempre la tua conferma, a meno che tu non l'abbia
  spenta per un'app che hai collegato tu.
- **Chiavi e password** restano nel portachiavi del telefono.
