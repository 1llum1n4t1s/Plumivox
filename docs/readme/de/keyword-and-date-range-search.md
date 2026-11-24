# Suche nach Stichwort und Zeitraum

Sie können zum Beispiel sagen: „Suche auf ganz X nach Beiträgen zur Spracherkennung vom 1. bis 7. Oktober“ oder „Suche mit denselben Bedingungen bei den Konten, denen blog_posting folgt“. Die Suche wird auf API-Seite eingegrenzt und liefert Text, Autor, Zeitpunkt und URL der Beiträge. Eine semantische Suche findet nicht statt.

`scope` von `search_tweets` ist `all` (Standard, öffentliche Suche auf ganz X) oder `following` (die gefolgten Konten des angegebenen Kontos). `keyword` ist erforderlich; der Ausdruck ohne führende und abschließende Leerzeichen wird als Phrase gesucht. Auch enthaltene `OR` oder `from:` werden nicht als Suchoperatoren behandelt. Ausdrücke mit Anführungszeichen, Backslashes oder Steuerzeichen werden abgelehnt.

`startTime` und `endTime` sind optionale Zeitpunkte im ISO-8601-Format mit Zeitzone und Sekundengenauigkeit. Gesucht wird in einem Zeitraum, der den Beginn einschließt und das Ende ausschließt. Ein reines Datum wird nicht angegeben; Codex wandelt Datum und Zeitzone der Anfrage in Zeitpunkte um. Für den 1. bis 7. Oktober in japanischer Zeit lauten die Bedingungen beispielsweise:

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

Die Suche ruft zuerst die neuesten Beiträge ab. Übergeben Sie `nextCursor` an `cursor` des nächsten Aufrufs, um die Fortsetzung mit denselben Bedingungen abzurufen. Wenn Sie Konto, Ziel, Stichwort, Zeitraum oder die Obergrenze der gefolgten Konten ändern, lassen Sie den Cursor weg und beginnen eine neue Suche. Auch nach einer Änderung des API-Schlüssels sind vorhandene Such-Cursor nicht verwendbar.

`following` kann `maxAccounts` (20 bis 200, Standard 100) und `followingCursor` verwenden. Wenn Sie `coverage.followingNextCursor` übergeben, wird die nächste Liste gefolgter Konten mit denselben Bedingungen durchsucht. Gleichzeitig mit `cursor` der Beitragssuche kann dies nicht angegeben werden. Bei `all` werden diese beiden Argumente nicht angegeben. Cursor des vorhandenen `get_following_feed` und der Suche sind nicht untereinander austauschbar. Auch Such-Cursor älterer Versionen für gefolgte Konten, die einen alten unsignierten Feed-Cursor enthalten, sind nicht verwendbar; lassen Sie den Cursor weg und suchen Sie erneut.

Die Suche auf ganz X benötigt eine API-Anfrage pro Seite, die Suche bei gefolgten Konten wie der vorhandene Feed höchstens 6 (100 Personen) bzw. 11 (200 Personen) API-Anfragen. Die Vollständigkeit geschützter Beiträge oder aller Beiträge im Suchzeitraum ist nicht garantiert; der Abrufumfang wird in `coverage` zurückgegeben. Die Zeitraumoperatoren werden gemäß der [Beschreibung von TwitterAPI.io](https://twitterapi.io/blog/search-a-tweet) in UTC umgerechnet.
