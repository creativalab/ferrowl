#if DEBUG
/// The sample mail of the demo mode (see `Demo`), in every app language.
extension Demo {
    static let texts: [String: [String: String]] = [
        "en": en, "it": it, "es": es, "fr": fr, "de": de, "pt": pt, "ja": ja, "ko": ko, "zh": zh, "ar": ar,
    ]

    private static let en: [String: String] = [
        "f_projects": "Projects",
        "f_travel": "Travel",
        "hike_subject": "Hike on Saturday?",
        "hike_1": "Hi Alex! The forecast for Saturday looks great. Shall we do the lake trail? We could leave at 8.",
        "hike_2": "Count me in! I can drive, there's room for four.",
        "hike_3": "Perfect, see you both at 8 at the station. I'll bring coffee ☕",
        "news_from": "Field Notes",
        "news_subject": "5 quiet places to read this weekend",
        "news_preview": "Plus: the best autumn walks in the city and a reading list for long evenings.",
        "news_title": "Five quiet places to read",
        "news_intro": "Autumn is here. We asked our readers where they go when they want an hour of peace with a good book.",
        "news_1_title": "The garden library",
        "news_1": "A glass reading room among the trees, open until late on weekdays.",
        "news_2_title": "The riverside café",
        "news_2": "Big windows, soft armchairs and the best hot chocolate in town.",
        "news_3_title": "The old observatory",
        "news_3": "Free entry on Sundays, with a reading corner under the dome.",
        "news_button": "Read the full issue",
        "news_footer": "You receive this newsletter because you subscribed at example.org. Unsubscribe",
        "pgp_subject": "Contract draft – signed copy",
        "pgp_preview": "Hi Alex, here is the final draft with my changes. I've signed and encrypted it…",
        "pgp_body": """
        Hi Alex,

        here is the final draft of the contract with the changes we discussed on Tuesday:

        • delivery moved to 15 January
        • payment in two instalments
        • one extra round of revisions

        I've signed and encrypted this message with OpenPGP, so only you can read it. If everything looks good, sign the PDF and send it back whenever you like.

        Best,
        Daniel
        """,
        "inv_subject": "Invitation: Design review",
        "inv_preview": "Yuki Tanaka has invited you to an event.",
        "inv_body": """
        Hi Alex,

        let's go through the new screens together before the release. I'll share the prototype at the start of the meeting.

        Yuki
        """,
        "inv_summary": "Design review",
        "inv_location": "Room 4B · 2nd floor",
        "lib_from": "City Library",
        "lib_subject": "Your reservation is ready",
        "lib_preview": "The book you reserved is waiting for you at the front desk until Friday.",
        "photos_subject": "Photos from the trip",
        "photos_preview": "Here are the best shots from Lisbon! The one at sunset is my favourite.",
        "budget_subject": "Quick question about the budget",
        "budget_preview": "Do you have five minutes today? I'd like to check two lines of the Q1 plan with you.",
        "dinner_subject": "Dinner on Friday?",
        "dinner_preview": "We're trying the new place near the market. Table for six at 8 pm, are you in?",
        "slides_subject": "Slides for Monday",
        "slides_preview": "Attached are the slides for the client meeting. Comments welcome by Sunday.",
        "coop_from": "Green Market",
        "coop_subject": "Your order is on its way",
        "coop_preview": "Your weekly vegetable box arrives tomorrow between 9 and 11 am.",
        "ev_standup": "Team stand-up",
        "ev_call": "Video call",
        "ev_lunch": "Lunch with Sofia",
        "ev_lunch_loc": "Riverside café",
        "ev_yoga": "Yoga",
        "ev_dentist": "Dentist",
        "ev_birthday": "Clara's birthday",
        "ev_hike": "Hike to the lake",
        "ev_hike_loc": "Central station",
    ]

    private static let it: [String: String] = [
        "f_projects": "Progetti",
        "f_travel": "Viaggi",
        "hike_subject": "Escursione sabato?",
        "hike_1": "Ciao Alex! Le previsioni per sabato sono ottime. Facciamo il sentiero del lago? Potremmo partire alle 8.",
        "hike_2": "Ci sto! Posso guidare io, c'è posto per quattro.",
        "hike_3": "Perfetto, ci vediamo alle 8 in stazione. Il caffè lo porto io ☕",
        "news_from": "Field Notes",
        "news_subject": "5 posti tranquilli dove leggere nel weekend",
        "news_preview": "E poi: le passeggiate d'autunno più belle in città e una lista di libri per le serate lunghe.",
        "news_title": "Cinque posti tranquilli dove leggere",
        "news_intro": "L'autunno è arrivato. Abbiamo chiesto ai lettori dove vanno quando vogliono un'ora di pace con un buon libro.",
        "news_1_title": "La biblioteca del giardino",
        "news_1": "Una sala di lettura in vetro tra gli alberi, aperta fino a tardi nei giorni feriali.",
        "news_2_title": "Il caffè sul fiume",
        "news_2": "Grandi vetrate, poltrone morbide e la cioccolata calda più buona della città.",
        "news_3_title": "Il vecchio osservatorio",
        "news_3": "Ingresso libero la domenica, con un angolo lettura sotto la cupola.",
        "news_button": "Leggi il numero completo",
        "news_footer": "Ricevi questa newsletter perché ti sei iscritto su example.org. Annulla l'iscrizione",
        "pgp_subject": "Bozza del contratto – copia firmata",
        "pgp_preview": "Ciao Alex, ecco la bozza finale con le mie modifiche. L'ho firmata e cifrata…",
        "pgp_body": """
        Ciao Alex,

        ecco la bozza finale del contratto con le modifiche di cui abbiamo parlato martedì:

        • consegna spostata al 15 gennaio
        • pagamento in due rate
        • un giro di revisioni in più

        Ho firmato e cifrato questo messaggio con OpenPGP, quindi solo tu puoi leggerlo. Se va tutto bene, firma il PDF e rimandamelo quando vuoi.

        A presto,
        Daniel
        """,
        "inv_subject": "Invito: Revisione del design",
        "inv_preview": "Yuki Tanaka ti ha invitato a un evento.",
        "inv_body": """
        Ciao Alex,

        guardiamo insieme le nuove schermate prima del rilascio. Condivido il prototipo all'inizio della riunione.

        Yuki
        """,
        "inv_summary": "Revisione del design",
        "inv_location": "Sala 4B · 2° piano",
        "lib_from": "Biblioteca civica",
        "lib_subject": "La tua prenotazione è pronta",
        "lib_preview": "Il libro che hai prenotato ti aspetta al banco prestiti fino a venerdì.",
        "photos_subject": "Le foto del viaggio",
        "photos_preview": "Ecco gli scatti migliori di Lisbona! Quello al tramonto è il mio preferito.",
        "budget_subject": "Domanda veloce sul budget",
        "budget_preview": "Hai cinque minuti oggi? Vorrei controllare con te due voci del piano del primo trimestre.",
        "dinner_subject": "Cena venerdì?",
        "dinner_preview": "Proviamo il locale nuovo vicino al mercato. Tavolo per sei alle 20, ci sei?",
        "slides_subject": "Le slide per lunedì",
        "slides_preview": "In allegato le slide per l'incontro con il cliente. Commenti entro domenica, grazie.",
        "coop_from": "Mercato Verde",
        "coop_subject": "Il tuo ordine è in arrivo",
        "coop_preview": "La cassetta di verdure della settimana arriva domani tra le 9 e le 11.",
        "ev_standup": "Riunione del team",
        "ev_call": "Videochiamata",
        "ev_lunch": "Pranzo con Sofia",
        "ev_lunch_loc": "Caffè sul fiume",
        "ev_yoga": "Yoga",
        "ev_dentist": "Dentista",
        "ev_birthday": "Compleanno di Clara",
        "ev_hike": "Escursione al lago",
        "ev_hike_loc": "Stazione centrale",
    ]

    private static let es: [String: String] = [
        "f_projects": "Proyectos",
        "f_travel": "Viajes",
        "hike_subject": "¿Excursión el sábado?",
        "hike_1": "¡Hola, Alex! El pronóstico para el sábado es estupendo. ¿Hacemos la ruta del lago? Podríamos salir a las 8.",
        "hike_2": "¡Me apunto! Puedo llevar el coche, hay sitio para cuatro.",
        "hike_3": "Perfecto, nos vemos a las 8 en la estación. Yo llevo el café ☕",
        "news_from": "Field Notes",
        "news_subject": "5 lugares tranquilos para leer este fin de semana",
        "news_preview": "Además: los mejores paseos de otoño por la ciudad y una lista de lecturas para las noches largas.",
        "news_title": "Cinco lugares tranquilos para leer",
        "news_intro": "Ya llegó el otoño. Preguntamos a nuestros lectores adónde van cuando quieren una hora de calma con un buen libro.",
        "news_1_title": "La biblioteca del jardín",
        "news_1": "Una sala de lectura de cristal entre los árboles, abierta hasta tarde entre semana.",
        "news_2_title": "El café junto al río",
        "news_2": "Grandes ventanales, sillones cómodos y el mejor chocolate caliente de la ciudad.",
        "news_3_title": "El antiguo observatorio",
        "news_3": "Entrada gratuita los domingos, con un rincón de lectura bajo la cúpula.",
        "news_button": "Leer el número completo",
        "news_footer": "Recibes este boletín porque te suscribiste en example.org. Darse de baja",
        "pgp_subject": "Borrador del contrato – copia firmada",
        "pgp_preview": "Hola, Alex: aquí tienes el borrador final con mis cambios. Lo he firmado y cifrado…",
        "pgp_body": """
        Hola, Alex:

        aquí tienes el borrador final del contrato con los cambios que comentamos el martes:

        • entrega aplazada al 15 de enero
        • pago en dos plazos
        • una ronda más de revisiones

        He firmado y cifrado este mensaje con OpenPGP, así que solo tú puedes leerlo. Si todo está bien, firma el PDF y devuélvemelo cuando puedas.

        Un saludo,
        Daniel
        """,
        "inv_subject": "Invitación: Revisión de diseño",
        "inv_preview": "Yuki Tanaka te ha invitado a un evento.",
        "inv_body": """
        Hola, Alex:

        repasemos juntos las nuevas pantallas antes del lanzamiento. Compartiré el prototipo al empezar la reunión.

        Yuki
        """,
        "inv_summary": "Revisión de diseño",
        "inv_location": "Sala 4B · 2.ª planta",
        "lib_from": "Biblioteca municipal",
        "lib_subject": "Tu reserva está lista",
        "lib_preview": "El libro que reservaste te espera en el mostrador de préstamos hasta el viernes.",
        "photos_subject": "Las fotos del viaje",
        "photos_preview": "¡Aquí van las mejores fotos de Lisboa! La del atardecer es mi favorita.",
        "budget_subject": "Una pregunta rápida sobre el presupuesto",
        "budget_preview": "¿Tienes cinco minutos hoy? Me gustaría revisar contigo dos partidas del plan del primer trimestre.",
        "dinner_subject": "¿Cenamos el viernes?",
        "dinner_preview": "Vamos a probar el sitio nuevo junto al mercado. Mesa para seis a las 20:00, ¿te apuntas?",
        "slides_subject": "Diapositivas para el lunes",
        "slides_preview": "Te adjunto las diapositivas para la reunión con el cliente. Comentarios antes del domingo, por favor.",
        "coop_from": "Mercado Verde",
        "coop_subject": "Tu pedido está en camino",
        "coop_preview": "Tu caja semanal de verduras llegará mañana entre las 9 y las 11.",
        "ev_standup": "Reunión del equipo",
        "ev_call": "Videollamada",
        "ev_lunch": "Comida con Sofia",
        "ev_lunch_loc": "Café junto al río",
        "ev_yoga": "Yoga",
        "ev_dentist": "Dentista",
        "ev_birthday": "Cumpleaños de Clara",
        "ev_hike": "Excursión al lago",
        "ev_hike_loc": "Estación central",
    ]

    private static let fr: [String: String] = [
        "f_projects": "Projets",
        "f_travel": "Voyages",
        "hike_subject": "Randonnée samedi ?",
        "hike_1": "Salut Alex ! La météo de samedi s'annonce superbe. On fait le sentier du lac ? On pourrait partir à 8 h.",
        "hike_2": "Je suis partant ! Je peux conduire, il y a de la place pour quatre.",
        "hike_3": "Parfait, rendez-vous à 8 h à la gare. J'apporte le café ☕",
        "news_from": "Field Notes",
        "news_subject": "5 endroits calmes pour lire ce week-end",
        "news_preview": "Et aussi : les plus belles balades d'automne en ville et des idées de lecture pour les longues soirées.",
        "news_title": "Cinq endroits calmes pour lire",
        "news_intro": "L'automne est là. Nous avons demandé à nos lecteurs où ils vont pour passer une heure tranquille avec un bon livre.",
        "news_1_title": "La bibliothèque du jardin",
        "news_1": "Une salle de lecture vitrée au milieu des arbres, ouverte tard en semaine.",
        "news_2_title": "Le café au bord de l'eau",
        "news_2": "De grandes baies vitrées, des fauteuils moelleux et le meilleur chocolat chaud de la ville.",
        "news_3_title": "L'ancien observatoire",
        "news_3": "Entrée libre le dimanche, avec un coin lecture sous la coupole.",
        "news_button": "Lire le numéro complet",
        "news_footer": "Vous recevez cette lettre car vous vous êtes inscrit sur example.org. Se désinscrire",
        "pgp_subject": "Projet de contrat – copie signée",
        "pgp_preview": "Salut Alex, voici la version finale avec mes modifications. Je l'ai signée et chiffrée…",
        "pgp_body": """
        Salut Alex,

        voici la version finale du contrat avec les modifications dont nous avons parlé mardi :

        • livraison repoussée au 15 janvier
        • paiement en deux fois
        • une série de corrections en plus

        J'ai signé et chiffré ce message avec OpenPGP : toi seul peux le lire. Si tout te convient, signe le PDF et renvoie-le-moi quand tu veux.

        À bientôt,
        Daniel
        """,
        "inv_subject": "Invitation : Revue du design",
        "inv_preview": "Yuki Tanaka vous a invité à un événement.",
        "inv_body": """
        Salut Alex,

        passons en revue ensemble les nouveaux écrans avant la sortie. Je partagerai le prototype au début de la réunion.

        Yuki
        """,
        "inv_summary": "Revue du design",
        "inv_location": "Salle 4B · 2e étage",
        "lib_from": "Bibliothèque municipale",
        "lib_subject": "Votre réservation est disponible",
        "lib_preview": "Le livre que vous avez réservé vous attend à l'accueil jusqu'à vendredi.",
        "photos_subject": "Les photos du voyage",
        "photos_preview": "Voici les meilleures photos de Lisbonne ! Celle au coucher du soleil est ma préférée.",
        "budget_subject": "Petite question sur le budget",
        "budget_preview": "Tu as cinq minutes aujourd'hui ? J'aimerais vérifier avec toi deux lignes du plan du premier trimestre.",
        "dinner_subject": "Dîner vendredi ?",
        "dinner_preview": "On teste le nouveau restaurant près du marché. Table pour six à 20 h, tu viens ?",
        "slides_subject": "Les slides pour lundi",
        "slides_preview": "Ci-joint les slides pour le rendez-vous client. Vos remarques avant dimanche, merci.",
        "coop_from": "Marché Vert",
        "coop_subject": "Votre commande est en route",
        "coop_preview": "Votre panier de légumes de la semaine arrive demain entre 9 h et 11 h.",
        "ev_standup": "Point d'équipe",
        "ev_call": "Visioconférence",
        "ev_lunch": "Déjeuner avec Sofia",
        "ev_lunch_loc": "Café au bord de l'eau",
        "ev_yoga": "Yoga",
        "ev_dentist": "Dentiste",
        "ev_birthday": "Anniversaire de Clara",
        "ev_hike": "Randonnée au lac",
        "ev_hike_loc": "Gare centrale",
    ]

    private static let de: [String: String] = [
        "f_projects": "Projekte",
        "f_travel": "Reisen",
        "hike_subject": "Wanderung am Samstag?",
        "hike_1": "Hallo Alex! Die Vorhersage für Samstag ist super. Sollen wir den Weg zum See nehmen? Wir könnten um 8 losfahren.",
        "hike_2": "Ich bin dabei! Ich kann fahren, im Auto ist Platz für vier.",
        "hike_3": "Perfekt, dann um 8 am Bahnhof. Ich bringe Kaffee mit ☕",
        "news_from": "Field Notes",
        "news_subject": "5 ruhige Orte zum Lesen am Wochenende",
        "news_preview": "Außerdem: die schönsten Herbstspaziergänge in der Stadt und Lesetipps für lange Abende.",
        "news_title": "Fünf ruhige Orte zum Lesen",
        "news_intro": "Der Herbst ist da. Wir haben unsere Leser gefragt, wohin sie gehen, wenn sie eine Stunde Ruhe mit einem guten Buch suchen.",
        "news_1_title": "Die Gartenbibliothek",
        "news_1": "Ein gläserner Lesesaal zwischen den Bäumen, werktags bis spät geöffnet.",
        "news_2_title": "Das Café am Fluss",
        "news_2": "Große Fenster, weiche Sessel und die beste heiße Schokolade der Stadt.",
        "news_3_title": "Die alte Sternwarte",
        "news_3": "Sonntags freier Eintritt, mit einer Leseecke unter der Kuppel.",
        "news_button": "Ganze Ausgabe lesen",
        "news_footer": "Du erhältst diesen Newsletter, weil du dich auf example.org angemeldet hast. Abmelden",
        "pgp_subject": "Vertragsentwurf – unterschriebene Kopie",
        "pgp_preview": "Hallo Alex, hier ist der finale Entwurf mit meinen Änderungen. Ich habe ihn signiert und verschlüsselt…",
        "pgp_body": """
        Hallo Alex,

        hier ist der finale Vertragsentwurf mit den Änderungen, über die wir am Dienstag gesprochen haben:

        • Lieferung auf den 15. Januar verschoben
        • Zahlung in zwei Raten
        • eine zusätzliche Korrekturrunde

        Ich habe diese Nachricht mit OpenPGP signiert und verschlüsselt, nur du kannst sie lesen. Wenn alles passt, unterschreib das PDF und schick es mir zurück, wann es dir passt.

        Viele Grüße
        Daniel
        """,
        "inv_subject": "Einladung: Design-Review",
        "inv_preview": "Yuki Tanaka hat dich zu einem Termin eingeladen.",
        "inv_body": """
        Hallo Alex,

        lass uns die neuen Screens vor dem Release gemeinsam durchgehen. Den Prototyp teile ich zu Beginn des Meetings.

        Yuki
        """,
        "inv_summary": "Design-Review",
        "inv_location": "Raum 4B · 2. Stock",
        "lib_from": "Stadtbibliothek",
        "lib_subject": "Deine Vormerkung liegt bereit",
        "lib_preview": "Das vorgemerkte Buch liegt bis Freitag an der Ausleihtheke für dich bereit.",
        "photos_subject": "Die Fotos von der Reise",
        "photos_preview": "Hier sind die besten Bilder aus Lissabon! Das beim Sonnenuntergang ist mein Favorit.",
        "budget_subject": "Kurze Frage zum Budget",
        "budget_preview": "Hast du heute fünf Minuten? Ich würde gern zwei Posten im Plan für das erste Quartal mit dir durchgehen.",
        "dinner_subject": "Abendessen am Freitag?",
        "dinner_preview": "Wir probieren das neue Lokal am Markt aus. Tisch für sechs um 20 Uhr, bist du dabei?",
        "slides_subject": "Folien für Montag",
        "slides_preview": "Anbei die Folien für den Kundentermin. Anmerkungen bitte bis Sonntag.",
        "coop_from": "Grüner Markt",
        "coop_subject": "Deine Bestellung ist unterwegs",
        "coop_preview": "Deine Gemüsekiste kommt morgen zwischen 9 und 11 Uhr.",
        "ev_standup": "Team-Stand-up",
        "ev_call": "Videoanruf",
        "ev_lunch": "Mittagessen mit Sofia",
        "ev_lunch_loc": "Café am Fluss",
        "ev_yoga": "Yoga",
        "ev_dentist": "Zahnarzt",
        "ev_birthday": "Claras Geburtstag",
        "ev_hike": "Wanderung zum See",
        "ev_hike_loc": "Hauptbahnhof",
    ]

    private static let pt: [String: String] = [
        "f_projects": "Projetos",
        "f_travel": "Viagens",
        "hike_subject": "Trilha no sábado?",
        "hike_1": "Oi, Alex! A previsão para sábado está ótima. Vamos fazer a trilha do lago? Podemos sair às 8.",
        "hike_2": "Tô dentro! Posso ir de carro, cabem quatro.",
        "hike_3": "Perfeito, a gente se vê às 8 na estação. Eu levo o café ☕",
        "news_from": "Field Notes",
        "news_subject": "5 lugares tranquilos para ler no fim de semana",
        "news_preview": "E mais: os melhores passeios de outono pela cidade e uma lista de leituras para as noites longas.",
        "news_title": "Cinco lugares tranquilos para ler",
        "news_intro": "O outono chegou. Perguntamos aos nossos leitores aonde eles vão quando querem uma hora de sossego com um bom livro.",
        "news_1_title": "A biblioteca do jardim",
        "news_1": "Uma sala de leitura de vidro entre as árvores, aberta até tarde nos dias úteis.",
        "news_2_title": "O café à beira do rio",
        "news_2": "Janelas enormes, poltronas macias e o melhor chocolate quente da cidade.",
        "news_3_title": "O antigo observatório",
        "news_3": "Entrada gratuita aos domingos, com um cantinho de leitura sob a cúpula.",
        "news_button": "Ler a edição completa",
        "news_footer": "Você recebe esta newsletter porque se inscreveu em example.org. Cancelar inscrição",
        "pgp_subject": "Minuta do contrato – cópia assinada",
        "pgp_preview": "Oi, Alex, segue a versão final com as minhas alterações. Assinei e criptografei…",
        "pgp_body": """
        Oi, Alex,

        segue a versão final do contrato com as mudanças que combinamos na terça:

        • entrega adiada para 15 de janeiro
        • pagamento em duas parcelas
        • uma rodada extra de revisões

        Assinei e criptografei esta mensagem com OpenPGP, então só você consegue lê-la. Se estiver tudo certo, assine o PDF e me devolva quando puder.

        Abraço,
        Daniel
        """,
        "inv_subject": "Convite: Revisão de design",
        "inv_preview": "Yuki Tanaka convidou você para um evento.",
        "inv_body": """
        Oi, Alex,

        vamos ver juntos as novas telas antes do lançamento. Compartilho o protótipo no início da reunião.

        Yuki
        """,
        "inv_summary": "Revisão de design",
        "inv_location": "Sala 4B · 2º andar",
        "lib_from": "Biblioteca Municipal",
        "lib_subject": "Sua reserva está disponível",
        "lib_preview": "O livro que você reservou está esperando no balcão de empréstimos até sexta-feira.",
        "photos_subject": "Fotos da viagem",
        "photos_preview": "Aqui estão as melhores fotos de Lisboa! A do pôr do sol é a minha favorita.",
        "budget_subject": "Uma pergunta rápida sobre o orçamento",
        "budget_preview": "Você tem cinco minutos hoje? Queria revisar com você dois itens do plano do primeiro trimestre.",
        "dinner_subject": "Jantar na sexta?",
        "dinner_preview": "Vamos experimentar o restaurante novo perto do mercado. Mesa para seis às 20h, topa?",
        "slides_subject": "Slides para segunda",
        "slides_preview": "Seguem em anexo os slides para a reunião com o cliente. Comentários até domingo, por favor.",
        "coop_from": "Feira Verde",
        "coop_subject": "Seu pedido está a caminho",
        "coop_preview": "Sua cesta semanal de legumes chega amanhã entre 9h e 11h.",
        "ev_standup": "Reunião diária da equipe",
        "ev_call": "Videochamada",
        "ev_lunch": "Almoço com a Sofia",
        "ev_lunch_loc": "Café à beira do rio",
        "ev_yoga": "Ioga",
        "ev_dentist": "Dentista",
        "ev_birthday": "Aniversário da Clara",
        "ev_hike": "Trilha até o lago",
        "ev_hike_loc": "Estação central",
    ]

    private static let ja: [String: String] = [
        "f_projects": "プロジェクト",
        "f_travel": "旅行",
        "hike_subject": "土曜日にハイキングはどう？",
        "hike_1": "アレックスさん、こんにちは！土曜日の天気予報はばっちりです。湖のコースに行きませんか？8時出発でどうでしょう。",
        "hike_2": "参加します！車を出せますよ、4人まで乗れます。",
        "hike_3": "よかった！じゃあ8時に駅で。コーヒーは私が持っていきます ☕",
        "news_from": "Field Notes",
        "news_subject": "週末に読書できる静かな場所5選",
        "news_preview": "ほかにも：街の秋のおすすめ散歩道と、長い夜のための読書リスト。",
        "news_title": "読書にぴったりの静かな場所5選",
        "news_intro": "秋がやってきました。良い本と一緒に静かな1時間を過ごしたいとき、どこへ行くのか読者のみなさんに聞いてみました。",
        "news_1_title": "庭の図書館",
        "news_1": "木々に囲まれたガラス張りの閲覧室。平日は夜遅くまで開いています。",
        "news_2_title": "川辺のカフェ",
        "news_2": "大きな窓とふかふかの椅子、そして街でいちばんおいしいホットチョコレート。",
        "news_3_title": "古い天文台",
        "news_3": "日曜日は入場無料。ドームの下に読書コーナーがあります。",
        "news_button": "最新号をすべて読む",
        "news_footer": "example.org でご登録いただいたため、このニュースレターをお送りしています。配信停止",
        "pgp_subject": "契約書の草案（署名済み）",
        "pgp_preview": "アレックスさん、修正を反映した最終版の草案です。OpenPGPで署名・暗号化しました…",
        "pgp_body": """
        アレックスさん

        火曜日に話し合った変更を反映した、契約書の最終版の草案をお送りします。

        • 納品日を1月15日に変更
        • 支払いは2回に分割
        • 修正対応を1回追加

        このメッセージはOpenPGPで署名・暗号化しているので、読めるのはあなただけです。問題がなければ、PDFに署名してご都合のよいときに返送してください。

        よろしくお願いします。
        ダニエル
        """,
        "inv_subject": "招待：デザインレビュー",
        "inv_preview": "Yuki Tanaka さんがあなたをイベントに招待しました。",
        "inv_body": """
        アレックスさん

        リリース前に、新しい画面を一緒に確認しましょう。プロトタイプは会議の冒頭で共有します。

        ユキ
        """,
        "inv_summary": "デザインレビュー",
        "inv_location": "4B会議室・2階",
        "lib_from": "市立図書館",
        "lib_subject": "ご予約の本をご用意しました",
        "lib_preview": "ご予約いただいた本は、金曜日まで貸出カウンターでお受け取りいただけます。",
        "photos_subject": "旅行の写真",
        "photos_preview": "リスボンのベストショットを送るね！夕日の写真がいちばんのお気に入り。",
        "budget_subject": "予算について少し質問です",
        "budget_preview": "今日5分ほどお時間ありますか？第1四半期の計画で2つの項目を一緒に確認したいです。",
        "dinner_subject": "金曜日に夕食はどう？",
        "dinner_preview": "市場の近くにできた新しいお店に行ってみます。20時に6人で予約したけど、来られる？",
        "slides_subject": "月曜日のスライド",
        "slides_preview": "顧客との打ち合わせ用のスライドを添付します。日曜日までにコメントをお願いします。",
        "coop_from": "グリーンマーケット",
        "coop_subject": "ご注文の商品を発送しました",
        "coop_preview": "今週の野菜セットは明日9時から11時の間にお届けします。",
        "ev_standup": "チーム朝会",
        "ev_call": "ビデオ通話",
        "ev_lunch": "ソフィアとランチ",
        "ev_lunch_loc": "川辺のカフェ",
        "ev_yoga": "ヨガ",
        "ev_dentist": "歯医者",
        "ev_birthday": "クララの誕生日",
        "ev_hike": "湖までハイキング",
        "ev_hike_loc": "中央駅",
    ]

    private static let ko: [String: String] = [
        "f_projects": "프로젝트",
        "f_travel": "여행",
        "hike_subject": "토요일에 등산 갈래?",
        "hike_1": "안녕 알렉스! 토요일 날씨가 아주 좋대. 호수 코스로 갈까? 8시에 출발하면 될 것 같아.",
        "hike_2": "나도 갈래! 내가 운전할게, 네 명까지 탈 수 있어.",
        "hike_3": "좋아, 그럼 8시에 역에서 보자. 커피는 내가 챙겨 갈게 ☕",
        "news_from": "Field Notes",
        "news_subject": "주말에 책 읽기 좋은 조용한 곳 5곳",
        "news_preview": "그리고: 도시의 가을 산책 코스와 긴 밤을 위한 추천 도서 목록.",
        "news_title": "책 읽기 좋은 조용한 곳 다섯 군데",
        "news_intro": "가을이 왔습니다. 좋은 책과 함께 조용한 한 시간을 보내고 싶을 때 어디로 가는지 독자들에게 물어봤습니다.",
        "news_1_title": "정원 도서관",
        "news_1": "나무 사이에 자리한 유리 열람실로, 평일에는 늦게까지 문을 엽니다.",
        "news_2_title": "강변 카페",
        "news_2": "커다란 창, 푹신한 의자, 그리고 도시에서 가장 맛있는 핫초코.",
        "news_3_title": "옛 천문대",
        "news_3": "일요일은 무료 입장이며, 돔 아래에 독서 공간이 있습니다.",
        "news_button": "이번 호 전체 읽기",
        "news_footer": "example.org에서 구독하셨기 때문에 이 뉴스레터를 받고 계십니다. 구독 취소",
        "pgp_subject": "계약서 초안 – 서명본",
        "pgp_preview": "알렉스 님, 제 수정 사항을 반영한 최종 초안입니다. OpenPGP로 서명하고 암호화했어요…",
        "pgp_body": """
        알렉스 님,

        화요일에 이야기한 변경 사항을 반영한 계약서 최종 초안입니다.

        • 납품일 1월 15일로 변경
        • 대금은 2회 분할 지급
        • 수정 1회 추가

        이 메시지는 OpenPGP로 서명하고 암호화했기 때문에 알렉스 님만 읽을 수 있습니다. 문제가 없으면 PDF에 서명해서 편하실 때 보내 주세요.

        감사합니다.
        다니엘
        """,
        "inv_subject": "초대: 디자인 리뷰",
        "inv_preview": "Yuki Tanaka 님이 일정에 초대했습니다.",
        "inv_body": """
        알렉스 님,

        출시 전에 새 화면을 함께 검토해요. 프로토타입은 회의를 시작할 때 공유하겠습니다.

        유키
        """,
        "inv_summary": "디자인 리뷰",
        "inv_location": "4B 회의실 · 2층",
        "lib_from": "시립 도서관",
        "lib_subject": "예약하신 도서가 준비되었습니다",
        "lib_preview": "예약하신 책은 금요일까지 대출 데스크에서 찾아가실 수 있습니다.",
        "photos_subject": "여행 사진",
        "photos_preview": "리스본에서 찍은 베스트 사진들이야! 해 질 녘 사진이 제일 마음에 들어.",
        "budget_subject": "예산 관련 간단한 질문",
        "budget_preview": "오늘 5분 정도 시간 있으세요? 1분기 계획에서 두 항목을 같이 확인하고 싶어요.",
        "dinner_subject": "금요일에 저녁 먹을래?",
        "dinner_preview": "시장 근처에 새로 생긴 식당에 가 보려고. 저녁 8시에 6명 예약했는데, 올 수 있어?",
        "slides_subject": "월요일 발표 자료",
        "slides_preview": "고객 미팅용 슬라이드를 첨부합니다. 일요일까지 의견 부탁드려요.",
        "coop_from": "그린 마켓",
        "coop_subject": "주문하신 상품이 배송 중입니다",
        "coop_preview": "이번 주 채소 꾸러미는 내일 오전 9시에서 11시 사이에 도착합니다.",
        "ev_standup": "팀 스탠드업",
        "ev_call": "화상 통화",
        "ev_lunch": "소피아와 점심",
        "ev_lunch_loc": "강변 카페",
        "ev_yoga": "요가",
        "ev_dentist": "치과",
        "ev_birthday": "클라라 생일",
        "ev_hike": "호수 등산",
        "ev_hike_loc": "중앙역",
    ]

    private static let zh: [String: String] = [
        "f_projects": "项目",
        "f_travel": "旅行",
        "hike_subject": "周六去徒步吗？",
        "hike_1": "嗨，Alex！周六的天气预报很好。我们走湖边那条步道怎么样？可以 8 点出发。",
        "hike_2": "算我一个！我可以开车，能坐四个人。",
        "hike_3": "太好了，那 8 点车站见。咖啡我来带 ☕",
        "news_from": "Field Notes",
        "news_subject": "周末读书的 5 个安静去处",
        "news_preview": "另外：城里最美的秋日散步路线，以及适合漫漫长夜的书单。",
        "news_title": "五个适合读书的安静去处",
        "news_intro": "秋天来了。我们问了读者：想捧一本好书安静地待上一小时，他们会去哪里？",
        "news_1_title": "花园图书馆",
        "news_1": "树林间的玻璃阅览室，工作日开放到很晚。",
        "news_2_title": "河畔咖啡馆",
        "news_2": "大大的窗户、柔软的沙发，还有全城最好喝的热巧克力。",
        "news_3_title": "老天文台",
        "news_3": "周日免费参观，穹顶下设有阅读角。",
        "news_button": "阅读本期全文",
        "news_footer": "您收到此简报是因为您在 example.org 订阅了它。退订",
        "pgp_subject": "合同草案（已签名）",
        "pgp_preview": "Alex 你好，这是加入我修改意见的最终草案。我已用 OpenPGP 签名并加密……",
        "pgp_body": """
        Alex 你好：

        这是合同的最终草案，已加入我们周二讨论的修改：

        • 交付日期改为 1 月 15 日
        • 分两期付款
        • 增加一轮修改

        这封邮件已用 OpenPGP 签名并加密，只有你能阅读。如果没有问题，请在 PDF 上签字，方便时发回给我。

        祝好，
        Daniel
        """,
        "inv_subject": "邀请：设计评审",
        "inv_preview": "Yuki Tanaka 邀请您参加一个活动。",
        "inv_body": """
        Alex 你好：

        发布之前，我们一起过一遍新界面吧。会议开始时我会分享原型。

        Yuki
        """,
        "inv_summary": "设计评审",
        "inv_location": "4B 会议室 · 二楼",
        "lib_from": "市图书馆",
        "lib_subject": "您预约的图书已到馆",
        "lib_preview": "您预约的图书将在借阅台为您保留至周五。",
        "photos_subject": "旅行的照片",
        "photos_preview": "这是在里斯本拍的最佳照片！日落那张是我的最爱。",
        "budget_subject": "关于预算的一个小问题",
        "budget_preview": "你今天有五分钟吗？想和你核对一下第一季度计划里的两项。",
        "dinner_subject": "周五一起吃晚饭？",
        "dinner_preview": "我们想去市场旁边新开的那家店。晚上 8 点订了六人桌，你来吗？",
        "slides_subject": "周一用的幻灯片",
        "slides_preview": "附件是客户会议用的幻灯片。请在周日前提出意见。",
        "coop_from": "绿色市集",
        "coop_subject": "您的订单正在配送中",
        "coop_preview": "本周的蔬菜箱将于明天上午 9 点至 11 点送达。",
        "ev_standup": "团队站会",
        "ev_call": "视频通话",
        "ev_lunch": "和 Sofia 吃午饭",
        "ev_lunch_loc": "河畔咖啡馆",
        "ev_yoga": "瑜伽",
        "ev_dentist": "看牙医",
        "ev_birthday": "Clara 的生日",
        "ev_hike": "湖边徒步",
        "ev_hike_loc": "中央车站",
    ]

    private static let ar: [String: String] = [
        "f_projects": "المشاريع",
        "f_travel": "السفر",
        "hike_subject": "نزهة في الجبل يوم السبت؟",
        "hike_1": "مرحبًا أليكس! توقعات الطقس ليوم السبت رائعة. ما رأيك في مسار البحيرة؟ يمكننا الانطلاق في الثامنة.",
        "hike_2": "أنا معكم! يمكنني القيادة، في السيارة مكان لأربعة.",
        "hike_3": "ممتاز، نلتقي في الثامنة عند المحطة. سأحضر القهوة ☕",
        "news_from": "Field Notes",
        "news_subject": "5 أماكن هادئة للقراءة في عطلة نهاية الأسبوع",
        "news_preview": "وأيضًا: أجمل نزهات الخريف في المدينة وقائمة كتب للأمسيات الطويلة.",
        "news_title": "خمسة أماكن هادئة للقراءة",
        "news_intro": "لقد حلّ الخريف. سألنا قرّاءنا إلى أين يذهبون عندما يريدون ساعة من الهدوء مع كتاب جيد.",
        "news_1_title": "مكتبة الحديقة",
        "news_1": "قاعة قراءة زجاجية بين الأشجار، مفتوحة حتى وقت متأخر في أيام الأسبوع.",
        "news_2_title": "مقهى النهر",
        "news_2": "نوافذ كبيرة وكراسٍ مريحة وأفضل شوكولاتة ساخنة في المدينة.",
        "news_3_title": "المرصد القديم",
        "news_3": "الدخول مجاني أيام الأحد، مع ركن للقراءة تحت القبة.",
        "news_button": "اقرأ العدد كاملًا",
        "news_footer": "تصلك هذه النشرة لأنك اشتركت عبر example.org. إلغاء الاشتراك",
        "pgp_subject": "مسودة العقد – نسخة موقّعة",
        "pgp_preview": "مرحبًا أليكس، إليك المسودة النهائية مع تعديلاتي. لقد وقّعتها وشفّرتها…",
        "pgp_body": """
        مرحبًا أليكس،

        إليك المسودة النهائية للعقد مع التعديلات التي ناقشناها يوم الثلاثاء:

        • تأجيل التسليم إلى 15 يناير
        • الدفع على قسطين
        • جولة مراجعة إضافية

        لقد وقّعت هذه الرسالة وشفّرتها باستخدام OpenPGP، لذا لا يمكن لأحد غيرك قراءتها. إذا كان كل شيء على ما يرام، وقّع ملف PDF وأعده إليّ متى شئت.

        مع التحية،
        دانيال
        """,
        "inv_subject": "دعوة: مراجعة التصميم",
        "inv_preview": "دعتك Yuki Tanaka إلى حدث.",
        "inv_body": """
        مرحبًا أليكس،

        لنراجع معًا الشاشات الجديدة قبل الإصدار. سأشارك النموذج الأولي في بداية الاجتماع.

        يوكي
        """,
        "inv_summary": "مراجعة التصميم",
        "inv_location": "القاعة 4B · الطابق الثاني",
        "lib_from": "مكتبة المدينة",
        "lib_subject": "حجزك جاهز",
        "lib_preview": "الكتاب الذي حجزته بانتظارك عند مكتب الإعارة حتى يوم الجمعة.",
        "photos_subject": "صور الرحلة",
        "photos_preview": "إليك أجمل اللقطات من لشبونة! صورة الغروب هي المفضلة لديّ.",
        "budget_subject": "سؤال سريع عن الميزانية",
        "budget_preview": "هل لديك خمس دقائق اليوم؟ أود مراجعة بندين من خطة الربع الأول معك.",
        "dinner_subject": "عشاء يوم الجمعة؟",
        "dinner_preview": "سنجرّب المطعم الجديد قرب السوق. طاولة لستة أشخاص في الثامنة مساءً، هل ستأتي؟",
        "slides_subject": "شرائح العرض ليوم الاثنين",
        "slides_preview": "مرفق شرائح اجتماع العميل. أرجو إرسال ملاحظاتك قبل يوم الأحد.",
        "coop_from": "السوق الأخضر",
        "coop_subject": "طلبك في الطريق",
        "coop_preview": "ستصلك سلة الخضار الأسبوعية غدًا بين التاسعة والحادية عشرة صباحًا.",
        "ev_standup": "اجتماع الفريق اليومي",
        "ev_call": "مكالمة فيديو",
        "ev_lunch": "غداء مع صوفيا",
        "ev_lunch_loc": "مقهى النهر",
        "ev_yoga": "يوغا",
        "ev_dentist": "طبيب الأسنان",
        "ev_birthday": "عيد ميلاد كلارا",
        "ev_hike": "نزهة إلى البحيرة",
        "ev_hike_loc": "المحطة المركزية",
    ]
}
#endif
