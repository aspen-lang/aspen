use aspen::Diagnostics;

pub fn report(diagnostics: Diagnostics) {
    let mut groups: Vec<_> = diagnostics.group_by_source().into_iter().collect();

    groups.sort_by(|(a, _), (b, _)| a.cmp(&b));

    for (uri, diagnostics) in groups {
        println!("{}", uri);

        for diagnostic in diagnostics {
            println!("{}: {}", diagnostic.range(), diagnostic.message());
        }
    }
}