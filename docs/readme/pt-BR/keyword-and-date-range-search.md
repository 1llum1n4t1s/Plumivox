# Busca por palavra-chave e período

Você pode pedir, por exemplo, "busque em todo o X posts sobre reconhecimento de voz de 1º a 7 de outubro" ou "busque com as mesmas condições entre as contas seguidas por blog_posting". A filtragem é feita no lado da API, e são retornados o texto, o autor, a data e hora e a URL dos posts. Não é feita busca semântica.

O `scope` de `search_tweets` é `all` (padrão, busca pública em todo o X) ou `following` (contas seguidas pela conta indicada). `keyword` é obrigatório, e o termo, sem os espaços no início e no fim, é buscado como frase. Mesmo que inclua `OR` ou `from:`, eles não são tratados como operadores de busca. Termos com aspas, barras invertidas ou caracteres de controle são recusados.

`startTime` e `endTime` são opcionais e são data e hora em ISO 8601, com precisão de segundos e fuso horário. A busca cobre o período que inclui o início e exclui o fim. Não se indica apenas a data; o Codex converte a data e o fuso horário pedidos em data e hora. Por exemplo, de 1º a 7 de outubro no horário do Japão, as condições são as seguintes.

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

A busca obtém os posts a partir dos mais recentes. Passando `nextCursor` para o `cursor` da próxima chamada, obtém-se a continuação com as mesmas condições. Para mudar a conta, o alvo, a palavra-chave, o período ou o limite de contas seguidas, remova o cursor e inicie uma nova busca. Se a chave de API for trocada, os cursors de busca existentes também não poderão ser usados.

`following` aceita `maxAccounts` (20 a 200, padrão 100) e `followingCursor`. Passando `coverage.followingNextCursor`, o próximo trecho da lista de contas seguidas é buscado com as mesmas condições. Ele não pode ser indicado ao mesmo tempo que o `cursor` da busca de posts. Com `all`, esses dois argumentos não são indicados. Os cursors do `get_following_feed` existente e os da busca não são intercambiáveis. Cursors de busca entre contas seguidas de versões antigas que contêm um cursor de feed antigo sem assinatura também não podem ser usados; remova o cursor e busque novamente.

A busca em todo o X faz 1 requisição de API por página, e a busca entre contas seguidas faz, como o feed existente, no máximo 6 (100 contas) / 11 (200 contas) requisições de API. Não há garantia de cobertura de posts protegidos nem de todos os posts do período buscado, e a abrangência obtida é retornada em `coverage`. Os operadores de período são convertidos para UTC conforme a [explicação do TwitterAPI.io](https://twitterapi.io/blog/search-a-tweet).
