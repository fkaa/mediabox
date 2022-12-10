use anyhow::Context;

async fn run() -> anyhow::Result<()> {
    let mut parser = lexopt::Parser::from_env();

    let mut inputs = Vec::new();
    let mut mappings = Vec::new();

    use lexopt::Arg::*;

    while let Some(arg) = parser.next().context("Failed parsing arguments")? {
        match arg {
            Short('i') => {
                inputs.push(parser.value()?);
            }
            Long("map") => {
                mappings.push(parser.value()?);
            }
            _ => return Err(arg.unexpected()).context("Failed parsing arguments")?,
        }
    }

    dbg!(&inputs);
    dbg!(&mappings);

    /*let mut ios = Vec::new();
    for input in inputs {
        ios.push(Io::open_file(&input).await?);
    }

    let mut demuxers = Vec::new();
    for mut io in ios {
        let meta = mediabox::probe(&mut io).await?;
        demuxers.push(meta.create(io));
    }*/

    Ok(())
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{e:?}");
    }
}
