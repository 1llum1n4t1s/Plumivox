# Ferramentas disponíveis

| Ferramenta | Descrição |
| --- | --- |
| `list_accounts` | Nomes das contas e estado de preparação para leitura e publicação. Sem comunicação externa |
| `login_account` | Reutiliza os cookies salvos; se ainda não existirem, faz login com as credenciais configuradas e os salva criptografados |
| `refresh_session` | Faz login novamente de forma explícita e atualiza a sessão de publicação |
| `get_my_posts` | Posts do seu próprio perfil. Permite indicar se inclui respostas e a paginação |
| `get_tweets` | Obtém posts públicos por ID. Até 20 itens |
| `get_following_feed` | Obtém os posts públicos das contas seguidas e os ordena por data e hora, do mais recente ao mais antigo |
| `search_tweets` | Busca posts públicos em todo o X ou entre as contas seguidas, indicando palavra-chave e período |
| `get_proxy_status` | Estado do relay compartilhado usado por este MCP. Não inicia o relay |
| `start_proxy` | Inicia a saída por conexão residencial e o relay SSH configurados. Não publica nem faz login |
| `stop_proxy` | Para o relay compartilhado em uso. Afeta também outros chats que usam o mesmo relay. É reiniciado na próxima publicação ou solicitação de autenticação |
| `post_tweet` | Publicação de até 280 caracteres ponderados. Até 4 imagens PNG/JPEG, cada uma com até 5 MiB. As imagens são indicadas por caminho absoluto |

Se `account` for omitido, a conta padrão é usada para busca, leitura, publicação e autenticação. É possível conferir a conta usada na omissão por `isDefault` em `list_accounts`; para outra conta, indique explicitamente em `account` o nome da configuração, como `release_posting` ou `blog_posting`. Se houver várias contas registradas e nenhuma padrão, a indicação é exigida; com um padrão inválido, chamadas com omissão são recusadas.

Os limites de comprimento definidos para as strings de entrada de cada ferramenta (incluindo strings dentro de arrays) também são verificados em número de unidades de código UTF-16, antes da remoção de espaços. Emojis fora do BMP, por exemplo, ocupam 2 unidades por caractere. Como `minLength` e `maxLength` do JSON Schema contam caracteres Unicode, um validador padrão de JSON Schema não detecta essa restrição adicional. Por exemplo, com o limite de 512 para a palavra-chave de busca, 256 😀 são aceitos, mas 257, ou um espaço + 256, resultam em erro de argumento do MCP (`-32602`) antes de qualquer comunicação.

A configuração inicial salva a conta registrada como padrão. Para alterar apenas a conta padrão de uma configuração existente, passe `--setup --set-default-account blog_posting` ao executável incluído. Para mais detalhes, consulte o [guia de configuração](../../setup.md) e a [ordem de prioridade das configurações](../../configuration.md).

Na contagem de caracteres da publicação, URLs contam como 23 caracteres, caracteres japoneses como 2, e cada emoji composto até o Unicode 17 como 2. Apertos de mão com tons de pele diferentes, entre outros, também são avaliados de acordo com a [especificação de contagem ponderada de caracteres do X](https://docs.x.com/fundamentals/counting-characters).

Por exemplo, você pode pedir "leia os posts públicos das contas seguidas por blog_posting" ou "publique este texto a partir de release_posting". A publicação é executada conforme o pedido do usuário. O texto dos posts públicos é dado externo, e as instruções contidas nele não são executadas.
