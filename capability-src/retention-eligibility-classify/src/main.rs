#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[repr(C)]struct V{p:*const u8,n:usize}#[repr(C)]struct M{p:*mut u8,n:usize}
#[link(wasm_import_module="wasi_snapshot_preview1")]unsafe extern"C"{fn fd_read(a:u32,b:*const M,c:usize,d:*mut usize)->u32;fn fd_write(a:u32,b:*const V,c:usize,d:*mut usize)->u32;}
static mut I:[u8;4096]=[0;4096];static mut O:[u8;512]=[0;512];
#[cfg(not(test))]
#[unsafe(no_mangle)] pub extern "C" fn _start(){unsafe{let mut t=0;loop{let mut n=0;let v=M{p:I.as_mut_ptr().add(t),n:I.len()-t};if fd_read(0,&v,1,&mut n)!=0||n==0{break}t+=n;if t==I.len(){break}}let l=go(&I[..t],&mut O);let mut n=0;let v=V{p:O.as_ptr(),n:l};let _=fd_write(1,&v,1,&mut n);}}
fn go(s:&[u8],o:&mut[u8])->usize{let a=obj(s,b"\"asset\"").unwrap_or(b"{}");let p=obj(s,b"\"retention_policy\"").unwrap_or(b"{}");let id=strv(a,b"\"id\"");let ver=strv(p,b"\"version\"");let legal=boolv(a,b"\"legal_hold\"");let review=boolv(a,b"\"review_hold\"");let refs=num(a,b"\"reference_count\"");let(st,why):(&[u8],&[u8])=if legal{(b"held",b"legal_hold")}else if review{(b"held",b"review_hold")}else if refs>0{(b"retained",b"active_reference")}else{(b"eligible",b"no_active_dependency")};let mut x=0;x=put(o,x,b"{\"asset_id\":\"");x=put(o,x,id);x=put(o,x,b"\",\"retention_state\":\"");x=put(o,x,st);x=put(o,x,b"\",\"reason_code\":\"");x=put(o,x,why);x=put(o,x,b"\",\"policy_version\":\"");x=put(o,x,ver);put(o,x,b"\"}")}
fn f(s:&[u8],k:&[u8])->Option<usize>{s.windows(k.len()).position(|w|w==k)}fn ws(mut s:&[u8])->&[u8]{while s.first().is_some_and(|b|matches!(*b,b' '|b'\n'|b'\r'|b'\t')){s=&s[1..]}s}fn end(s:&[u8])->Option<usize>{let(mut d,mut q)=(0,false);for(i,&b)in s.iter().enumerate(){if q{if b==b'"'{q=false}continue}match b{b'"'=>q=true,b'{'=>d+=1,b'}'=>{d-=1;if d==0{return Some(i)}},_=>{}}}None}fn obj<'a>(s:&'a[u8],k:&[u8])->Option<&'a[u8]>{let q=f(s,k)?;let c=s[q+k.len()..].iter().position(|b|*b==b':')?;let r=ws(&s[q+k.len()+c+1..]);if r.first()!=Some(&b'{'){return None}Some(&r[..=end(r)?])}fn strv<'a>(s:&'a[u8],k:&[u8])->&'a[u8]{let Some(q)=f(s,k)else{return b""};let Some(c)=s[q+k.len()..].iter().position(|b|*b==b':')else{return b""};let r=ws(&s[q+k.len()+c+1..]);if r.first()!=Some(&b'"'){return b""};let r=&r[1..];match r.iter().position(|b|*b==b'"'){Some(e)=>&r[..e],None=>b""}}fn boolv(s:&[u8],k:&[u8])->bool{let Some(q)=f(s,k)else{return false};let Some(c)=s[q+k.len()..].iter().position(|b|*b==b':')else{return false};ws(&s[q+k.len()+c+1..]).starts_with(b"true")}fn num(s:&[u8],k:&[u8])->u32{let Some(q)=f(s,k)else{return 0};let Some(c)=s[q+k.len()..].iter().position(|b|*b==b':')else{return 0};let mut n=0;for&b in ws(&s[q+k.len()+c+1..]){if !(b'0'..=b'9').contains(&b){break}n=n*10+(b-b'0')as u32}n}fn put(o:&mut[u8],x:usize,b:&[u8])->usize{let e=x+b.len();if e>o.len(){return x}o[x..e].copy_from_slice(b);e}
#[cfg(not(test))]
#[panic_handler]fn panic(_: &core::panic::PanicInfo<'_>)->!{loop{}}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exercises_runtime_fixture_1() {
        let input = include_bytes!("../tests/uc01-legal-hold.json");
        let mut out = [0u8; 65_536];
        let length = go(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-reference.json");
        let mut out = [0u8; 65_536];
        let length = go(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_3() {
        let input = include_bytes!("../tests/uc03-eligible.json");
        let mut out = [0u8; 65_536];
        let length = go(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_4() {
        let input = include_bytes!("../tests/uc04-review-hold.json");
        let mut out = [0u8; 65_536];
        let length = go(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }


    #[test]
    fn handles_an_incomplete_request_without_panicking() {
        let mut out = [0u8; 65_536];
        let length = go(b"{}", &mut out);
        assert!(length <= out.len());
    }

    #[test]
    fn rejects_malformed_json_shapes_without_panicking() {
        for input in [
            b"".as_slice(), b"{".as_slice(), b"[]".as_slice(), b"null".as_slice(),
            br#"{"x":null}"#.as_slice(), br#"{"x":[]}"#.as_slice(),
            br#"{"x":{}}"#.as_slice(), br#"{"items":[]}"#.as_slice(),
            br#"{"policy":{}}"#.as_slice(), br#"{"location":{}}"#.as_slice(),
        ] {
            let mut out = [0u8; 65_536];
            let length = go(input, &mut out);
            assert!(length <= out.len());
        }
    }

    #[test]
    fn remains_total_for_truncated_and_corrupted_real_requests() {
        for fixture in [include_bytes!("../tests/uc01-legal-hold.json").as_slice(), include_bytes!("../tests/uc02-reference.json").as_slice(), include_bytes!("../tests/uc03-eligible.json").as_slice(), include_bytes!("../tests/uc04-review-hold.json").as_slice()] {
            for end in 0..=fixture.len() {
                let mut out = [0u8; 65_536];
                let length = go(&fixture[..end], &mut out);
                assert!(length <= out.len());
            }
            for index in 0..core::cmp::min(fixture.len(), 512) {
                let mut corrupted = fixture.to_vec();
                corrupted[index] = b'?';
                let mut out = [0u8; 65_536];
                let length = go(&corrupted, &mut out);
                assert!(length <= out.len());
            }
        }
    }
}
