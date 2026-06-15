# Marreta Lang — agent guide   (Generated for Marreta vX.Y.Z)
> Full, live reference: https://marreta.dev/llms-full.txt

Marreta is a DSL for REST APIs. It is not Python, Ruby, or JavaScript. Write Marreta, not a framework.

## Get this right (do not write it like Python/Ruby/JS)

1. No imports, no framework, no app object. A route is a top-level declaration, its body is indented.
       route GET "/ping"
           reply 200, { ok: true }

2. The request is exposed as params, query, payload, headers, and auth, accessed directly. There is
   no req/request/ctx object and no request.json().
       account = doc.accounts.find(params.id)
       owner = auth.subject

3. Validate input with a schema and take payload as. An invalid body returns 422 automatically.
   Do not parse or validate by hand, and do not import a validator.
       schema NewAccount
           owner: string

       route POST "/accounts" take payload as NewAccount
           account = doc.accounts.save({ owner: payload.owner })
           reply 201, account

4. Infrastructure is built-in namespaces (db. doc. cache. queue. topic. http_client.), configured in
   marreta.env. Never construct a client or a connection in code.
       cache.set("rate:usd", 5)

5. Respond with reply STATUS, body. Guard with require X else fail STATUS, "msg".
   There is no return, no raise, no res.json.
       require account else fail 404, "account not found"
       reply 200, account

6. Queries are pipelines with >>, not chained methods.
       product = db.products >> where(sku: params.sku) >> fetch_one

7. Types are keywords. A collection is list of X (not [X] or List[X]), an optional field is name?: type.
       schema Search
           term: string
           tags: list of string
           limit?: integer

8. Tests live in the language: scenario / given / when / then, run in memory. No pytest, no jest.
       scenario "returns an account"
           given doc.accounts.find("a1") returns { owner: "Ana" }
           when GET "/accounts/a1"
           then status 200

## Core syntax (cheat)
route VERB "path" [take payload as Schema] · reply STATUS, body · fail STATUS, "msg" · require COND else ...
schema Name / field: type / field?: type / list of type · match X / if / else · and or not
db. doc. cache. queue. topic. http_client. · pipelines with >> (where, order, fetch, fetch_one)

## When you need more
Anything not here, or version-specific (full namespace/method list, OpenAPI, auth, migrations):
fetch https://marreta.dev/llms-full.txt
