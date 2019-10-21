# rendersvgsrv

*rendersvgsrv* is an [SVG](https://en.wikipedia.org/wiki/Scalable_Vector_Graphics) rendering application.

It starts a webserver on port 7878 which serves requests:

Example:

```
curl --data-ascii '--jpeg|80|car.svg|car.jpg' http://127.0.0.1:7878/convert
```



## Build

```bash
# build with a cairo backend
cargo build --release --features="cairo-backend"
```

See [BUILD.adoc](../../BUILD.adoc) for details.

## License

*rendersvg* is licensed under the [MPLv2.0](https://www.mozilla.org/en-US/MPL/).
