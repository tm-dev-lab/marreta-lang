# Omni Hub — Review Consolidado da Implementação

Este documento consolida:

- achados do outro dev
- achados da minha implementação/validação
- classificação final do que é:
  - bug do que a linguagem/runtime já oferece
  - feature faltante
  - limitação do harness/teste
  - observabilidade/infra
  - ponto já resolvido pela linguagem atual

## Resumo Executivo

O `examples/omni_hub` foi implementado e validado end-to-end com:

- Postgres
- MongoDB
- Redis
- RabbitMQ

O harness em [test.sh](/home/thiago/Dev/Git/marreta-lang/examples/omni_hub/test.sh) fecha o fluxo principal:

- criação da OS no relacional
- publicação de evento de tópico
- read-through cache
- fechamento da OS
- snapshot imutável em documento
- invalidação de cache
- mensagem de cobrança estacionada em fila

Resultado atual do harness:

- `20 passed, 0 failed`

Mesmo com o fluxo verde, surgiram alguns gaps reais da linguagem/runtime e alguns pontos que são só de infraestrutura/testabilidade.

Desde a primeira rodada deste review, os seguintes bugs já foram corrigidos nesta branch:

- `queue.push` agora declara fila nomeada automaticamente antes do publish
- `migrate diff` e `migrate generate` agora funcionam em modo source-first, sem depender de DB online
- `db.update(id inexistente)` agora retorna `null`, e a mesma semântica vale no runtime real e no scenario/mock layer

## Achados Consolidados

Neste review:

- `Bug` = a linguagem/runtime já oferece o conceito, mas o comportamento atual está incorreto, incompleto ou desalinhado com a expectativa natural de uso
- `Feature faltante` = a capacidade ainda não existe na linguagem e depende de decisão de produto/especificação

### 1. Ausência de `if/else` block

Origem:

- outro dev
- confirmado por mim

Status:

- resolvido pela linguagem atual
- achado histórico, não gap aberto

O que observamos:

Hoje a linguagem suporta:

- `x = value if cond`
- `require ... else fail`
- `match`

Mas não suporta bloco estilo:

```marreta
if cached
    reply 200, cached
else
    reply 200, fresh
```

Na primeira versão do `omni_hub`, o read-through cache foi expresso com:

```marreta
cached = cache.get(key)
cache_log = print("cache hit for #{key}") if cached
details = cached or load_order_details(params.id)
```

Isso resolveu o caso específico de forma aceitável, porque a regra era simples:

- se existe cache, usa cache
- senão, carrega do banco

Mas isso não cobria branching geral com ergonomia equivalente.

Resolução aplicada:

- a linguagem agora suporta `if/else` block como expressão
- o `omni_hub` foi atualizado para usar `if/else` diretamente no fluxo de cache
- o workaround com `cached or load_order_details(...)` deixou de ser necessário

Classificação final:

- ponto resolvido
- remover do backlog aberto do `omni_hub`

### 2. `db.update()` inexistente não propaga `null` de forma amigável no cenário mock

Origem:

- outro dev
- confirmado no código do runner de cenário

Status:

- válido, mas com escopo importante
- `Bug`
- corrigido nesta branch

O que observamos:

No runner de cenários, `update_by_id` do mock passa por `value_to_row(...)`, que exige mapa:

- `src/scenario_tests.rs`
- contexto: `"db update given"`

Ou seja: se o `given db.X.update returns ...` devolver `null`, isso explode antes do `require`.

Isso é diferente de um comportamento desejável como:

- update de ID inexistente retornar `null`
- a rota decidir com `require updated else fail 404, ...`

Classificação:

- `Bug`

Como resolver:

Opção A:

- no cenário mock, permitir `null` em `db.update`
- devolver `Value::Null` em vez de exigir `DbRow`

Opção B:

- padronizar a semântica da linguagem:
  - `db.update(id, data)` retorna registro atualizado
  - ou `null` se não existir

Resolução aplicada:

- a semântica foi fechada como `update inexistente -> null`
- Postgres real agora retorna `None`/`null` quando `UPDATE ... RETURNING *` não encontra linha
- o scenario/mock layer foi alinhado para aceitar `given db.X.update returns null`
- o `omni_hub` prova isso end-to-end com `PATCH /customers/999999/name -> 404`

### 3. `/_health` built-in não participa do route registry do `marreta test`

Origem:

- outro dev
- confirmado por mim

Status:

- válido
- `Feature faltante` de testabilidade, não bug do endpoint

O que observamos:

O endpoint `/_health` é montado diretamente no servidor em:

- [server.rs](/home/thiago/Dev/Git/marreta-lang/src/server.rs)

Ele não nasce de uma `route` carregada pelo `RouteRegistry`, então o `scenario_tests` não o enxerga.

Por isso ele hoje é testável:

- via HTTP real / curl / harness shell

Mas não via DSL de scenario test.

Classificação:

- `Feature faltante`
- especificamente de testabilidade/cobertura

Como resolver:

Opção A:

- manter `/_health` como built-in fora da DSL
- e aceitar que o teste dele é sempre externo

Opção B:

- expor built-ins no route registry/simulator de teste

Minha leitura:

Baixa prioridade. O shell test cobre bem esse caso.

### 4. Falta de API de datetime / clock

Origem:

- outro dev
- refinado por mim

Status:

- resolvido pela linguagem atual
- achado histórico, não gap aberto

O que observamos:

O achado original dizia “sem timestamp nativo”. A formulação correta era outra.

A linguagem precisa de uma API de datetime/clock, não apenas de um `now()` isolado.

O gap real que apareceu no `omni_hub` é:

- falta uma forma oficial de obter o instante atual
- falta uma modelagem clara de valores temporais no nível da linguagem

Na primeira versão do `omni_hub`, o `completed_at` do snapshot ficou artificial:

```marreta
completed_at: "closed:#{order.id}"
```

Resolução aplicada:

- a linguagem agora expõe a API `time.*`
- o `omni_hub` passou a usar `time.now()` em criação e fechamento da OS
- `ServiceOrder`, `OrderStatusResponse`, `OrderDetails`, `BillingCommand` e `AuditSnapshot` agora usam `instant`
- o harness valida serialização canônica e persistência dos campos temporais

Classificação final:

- ponto resolvido
- remover do backlog aberto do `omni_hub`

### 5. Observabilidade de consumers

Origem:

- outro dev
- refinado por mim

Status:

- válido como observação operacional
- não é gap principal da linguagem neste momento
- no máximo `Feature faltante` futura, não prioridade agora

O que observamos:

Hoje usamos:

- `print(...)`
- inspeção do RabbitMQ
- inspeção do Redis / Mongo / Postgres

Para o `omni_hub`, isso foi suficiente para provar:

- tópico recebido
- fila estacionada
- cache hit/miss

Classificação:

- observabilidade/infra principalmente
- no máximo uma `Feature faltante` futura

Como resolver, se virar prioridade futura:

- structured logging
- hooks de inspeção
- métricas/consumer diagnostics

Minha leitura:

Para este ciclo, não colocaria como débito da linguagem.

### 6. “Não existe rescue em rotas”

Origem:

- outro dev

Status:

- inválido como achado factual

O que observamos:

`rescue` já existe na linguagem e já está coberto em examples funcionais:

- [core.marreta](/home/thiago/Dev/Git/marreta-lang/examples/functional_tests/routes/core.marreta)

Casos já exercitados:

- `expr rescue fallback`
- `>> rescue fail ...`
- `>> rescue` com bloco
- `db` error rescued

Então o problema não é “falta rescue”.

O ponto mais justo seria:

- ainda não usamos `rescue` no `omni_hub`
- e ainda não mostramos um padrão idiomático para `doc.save(...) rescue ...` em fluxo de infra

Classificação:

- achado rejeitado como gap de linguagem
- no máximo: lacuna de exemplo/documentação

Como resolver:

- se quisermos, adicionar uma rota/example específico mostrando `doc.* rescue ...`

### 7. `queue.push` não declara automaticamente a fila nomeada

Origem:

- meu achado

Status:

- válido
- `Bug`
- corrigido nesta branch

O que observamos:

No fluxo do `omni_hub`, `queue.push "process_billing"` não bastou para deixar a mensagem estacionada no RabbitMQ quando a fila ainda não existia.

Para o harness passar, precisei pré-declarar a fila no `test.sh` com `rabbitmqadmin`.

No código do runtime:

- `push()` publica no default exchange usando o nome da fila como routing key
- mas não chama `declare_queue(queue)` antes

Resultado:

- se a fila não existir previamente no broker, a mensagem não fica retida como o caso de uso espera

Classificação:

- `Bug`

Como resolver:

Opção A:

- `queue.push` declarar a fila durável automaticamente antes do publish

Opção B:

- expor um `queue.declare("name")` explícito na linguagem

Minha leitura:

Eu corrigiria no runtime. Para point-to-point, a semântica mais intuitiva é a fila existir por convenção.

Resolução aplicada:

- o runtime RabbitMQ agora chama `declare_queue(queue)` antes do `basic_publish` de `queue.push`
- o `omni_hub` deixou de pré-declarar a fila manualmente no harness
- a mensagem continua ficando estacionada no broker, comprovada por inspeção direta no RabbitMQ

### 8. `migrate diff` / `migrate generate` ainda dependem de banco acessível

Origem:

- meu achado

Status:

- válido
- `Bug` de DX/CLI no estado atual
- corrigido nesta branch

O que observamos:

No `omni_hub`, fora da infra no ar, `migrate diff` / `migrate generate` tentam conectar no banco e não seguem apenas com o source local.

Isso apareceu logo no primeiro fluxo manual.

Classificação:

- `Bug`

Como resolver:

Opção A:

- `diff` e `generate` funcionarem em modo source-only quando possível

Opção B:

- separar claramente:
  - `generate` local
  - `status/apply/rollback` conectados

Minha leitura:

Vale corrigir. Para DX, `generate` não deveria depender de DB online.

Resolução aplicada:

- `diff` e `generate` deixaram de usar introspecção remota como baseline
- agora o baseline vem da reconstrução do schema a partir das migrations locais
- `apply`, `rollback` e `status` continuam conectados, como devem ser
- o `omni_hub` prova que:
  - `migrate diff` funciona sem infra no ar
  - `migrate generate` funciona sem infra no ar

## Consolidação Final

### Bugs

1. nenhum bug aberto remanescente deste ciclo do `omni_hub`

### Features faltantes

1. cobertura de `/_health` no scenario runner, se quisermos isso
2. observabilidade mais rica de consumers, se isso virar prioridade

### Gaps de testabilidade / tooling

1. `/_health` não entra no route registry do `marreta test`

### Observações de infra, não da linguagem

1. observabilidade de consumers via logs/containers ainda é suficiente para este estágio

### Achados rejeitados como “gap de linguagem”

1. “não existe rescue em rotas”
   - rejeitado: `rescue` já existe e funciona

## Prioridade sugerida

### Alta

1. nenhuma pendência crítica de bug aberta neste ciclo

### Média

1. expor `/_health` no scenario runner, se isso de fato virar prioridade
2. ampliar examples com `rescue` em operações de infra
### Baixa

1. observabilidade mais rica de consumers, se isso se tornar prioridade
