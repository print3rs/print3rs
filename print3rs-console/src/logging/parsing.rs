use crate::commands::Command;
use winnow::{
    ascii::{alphanumeric1, float, space1},
    combinator::{alt, delimited, dispatch, empty, fail, preceded, repeat, rest},
    prelude::*,
    stream::AsChar,
    token::{take, take_till, take_until},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Segment<'a> {
    Tag(&'a str),
    Escaped(char),
    Value(&'a str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum OwnedSegment {
    Tag(String),
    Escaped(char),
    Value,
}

impl<'a> From<Segment<'a>> for OwnedSegment {
    fn from(value: Segment<'a>) -> Self {
        match value {
            Segment::Tag(s) => OwnedSegment::Tag(s.to_string()),
            Segment::Escaped(c) => OwnedSegment::Escaped(c),
            Segment::Value(_) => OwnedSegment::Value,
        }
    }
}

fn parse_tag<'a>(input: &mut &'a str) -> PResult<Segment<'a>> {
    Ok(Segment::Tag(take_till(1.., ('{', '}')).parse_next(input)?))
}

fn parse_escape<'a>(input: &mut &'a str) -> PResult<Segment<'a>> {
    dispatch! {take(2usize);
    "{{" => empty.map(|_| Segment::Escaped('{')),
    "}}" => empty.map(|_| Segment::Escaped('}')),
    _ => fail,
    }
    .parse_next(input)
}

fn parse_value<'a>(input: &mut &'a str) -> PResult<Segment<'a>> {
    Ok(Segment::Value(
        delimited("{", alphanumeric1.recognize(), "}").parse_next(input)?,
    ))
}

fn parse_segment<'a>(input: &mut &'a str) -> PResult<Segment<'a>> {
    alt((parse_tag, parse_escape, parse_value)).parse_next(input)
}

pub fn parse_segments<'a>(input: &mut &'a str) -> PResult<Vec<Segment<'a>>> {
    repeat(1.., parse_segment).parse_next(input)
}

pub fn parse_logger<'a>(input: &mut &'a str) -> PResult<Command<'a>> {
    (
        preceded(space1, alphanumeric1),
        preceded(space1, parse_segments),
    )
        .map(|(name, segments)| Command::Log(name, segments))
        .parse_next(input)
}

pub fn make_parser(segments: Vec<Segment<'_>>) -> impl FnMut(&mut &[u8]) -> PResult<Vec<f32>> {
    let segments = segments
        .into_iter()
        .map(|segment| OwnedSegment::from(segment.to_owned()))
        .collect::<Vec<_>>();
    move |input: &mut &[u8]| -> PResult<Vec<f32>> {
        let mut values = vec![];

        // skips up to pattern start
        if let Some(first) = segments.first() {
            match first {
                OwnedSegment::Tag(tag) => {
                    take_until(0.., tag.as_bytes()).void().parse_next(input)?;
                }
                OwnedSegment::Escaped(c) => {
                    take_till(0.., |i| (*c as u8) == i)
                        .void()
                        .parse_next(input)?;
                }
                OwnedSegment::Value => {
                    take_till(0.., |i: u8| i.is_dec_digit() || [b'.', b'-'].contains(&i))
                        .void()
                        .parse_next(input)?;
                }
            };
        }
        for segment in segments.iter() {
            match segment {
                OwnedSegment::Tag(ref s) => {
                    s.as_bytes().parse_next(input)?;
                }
                OwnedSegment::Escaped(mut c) => {
                    c.parse_next(input)?;
                }
                OwnedSegment::Value => {
                    values.push(float.parse_next(input)?);
                }
            };
        }
        // ignores rest of pattern
        rest.parse_next(input)?;
        Ok(values)
    }
}

pub fn get_headers(segments: &[Segment]) -> String {
    let mut s = String::new();
    for segment in segments {
        if let Segment::Value(label) = segment {
            s.push_str(label);
            s.push(',');
        }
    }
    // strip trailing
    if s.ends_with(',') {
        s.pop();
    };
    s.push('\n');
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use Segment::*;

    #[test]
    fn test_parse_segments() {
        let input = " this {is}so12.?me{segm2ents}";
        let expected: &[Segment] = &[
            Tag(" this "),
            Value("is"),
            Tag("so12.?me"),
            Value("segm2ents"),
        ];
        let parsed = parse_segments.parse(input).unwrap();
        assert_eq!(expected, parsed);
    }

    #[test]
    fn test_headers() {
        let segments = [Tag("one"), Value("two"), Tag("three"), Value("four")];
        let headers = get_headers(&segments);
        assert_eq!(&headers, "two,four,");
    }

    #[test]
    fn test_parsed_parser() {
        let parse_pattern = "millis: {millis},pos:{pos},current:{current}";
        let segments = parse_segments.parse(parse_pattern).unwrap();
        let mut parser = make_parser(segments);
        let final_out = parser
            .parse(b"millis: 1234.5,pos:-4.0,current:100")
            .unwrap();
        assert_eq!(final_out, vec![1234.5, -4.0, 100.0]);
    }

    #[test]
    fn test_escaped_braces() {
        let parse_pattern = "some{{nested:{stuff}}}";
        let segments = parse_segments.parse(parse_pattern).unwrap();
        assert_eq!(
            segments,
            vec![
                Segment::Tag("some"),
                Segment::Escaped('{'),
                Segment::Tag("nested:"),
                Segment::Value("stuff"),
                Segment::Escaped('}')
            ]
        );
    }

    #[test]
    fn test_ignores_rest() {
        let parse_pattern = "millis: {millis},pos:{pos},current:{current}";
        let segments = parse_segments.parse(parse_pattern).unwrap();
        let mut parser = make_parser(segments);
        let final_out = parser
            .parse(b"a bunch of stuff{}{}{{}}.028millis: 1234.5,pos:-4.0,current:100,and a bunch of other stuff{}{}{{}}.028")
            .unwrap();
        assert_eq!(final_out, vec![1234.5, -4.0, 100.0]);
    }
}
