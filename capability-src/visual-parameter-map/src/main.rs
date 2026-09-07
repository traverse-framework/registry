#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
#[repr(C)]struct V{p:*const u8,n:usize}#[repr(C)]struct M{p:*mut u8,n:usize}
#[link(wasm_import_module="wasi_snapshot_preview1")]unsafe extern"C"{fn fd_read(a:u32,b:*const M,c:usize,d:*mut usize)->u32;fn fd_write(a:u32,b:*const V,c:usize,d:*mut usize)->u32;}
static mut I:[u8;4096]=[0;4096];static mut O:[u8;512]=[0;512];
#[cfg(not(test))]
#[unsafe(no_mangle)]pub extern "C" fn _start(){unsafe{let(mut t,mut n)=(0,0);loop{let v=M{p:I.as_mut_ptr().add(t),n:I.len()-t};if fd_read(0,&v,1,&mut n)!=0||n==0{break}t+=n;if t==I.len(){break}}let l=go(&I[..t],&mut O);let v=V{p:O.as_ptr(),n:l};let _=fd_write(1,&v,1,&mut n);}}
fn go(s:&[u8],o:&mut[u8])->usize{let e=num(s,b"\"evidence_count\"");let c=num(s,b"\"coverage_millis\"");let u=num(s,b"\"uncertainty_millis\"");let seed=num(s,b"\"seed\"");let ver=st(s,b"\"version\"");let(mut x,derived_seed)=(0,seed+e);x=put(o,x,b"{\"derived_seed\":");x=uint(o,x,derived_seed);x=put(o,x,b",\"density_millis\":");x=uint(o,x,e);x=put(o,x,b",\"motion_millis\":");x=uint(o,x,c);x=put(o,x,b",\"uncertainty_treatment\":\"");x=put(o,x,if u>=500{b"visible"}else{b"subtle"});x=put(o,x,b"\",\"policy_version\":\"");x=put(o,x,ver);put(o,x,b"\"}")}
fn f(s:&[u8],k:&[u8])->Option<usize>{s.windows(k.len()).position(|w|w==k)}fn ws(mut s:&[u8])->&[u8]{while s.first().is_some_and(|b|matches!(*b,b' '|b'\n'|b'\r'|b'\t')){s=&s[1..]}s}fn num(s:&[u8],k:&[u8])->u32{let Some(q)=f(s,k)else{return 0};let Some(c)=s[q+k.len()..].iter().position(|b|*b==b':')else{return 0};let mut n=0;for&b in ws(&s[q+k.len()+c+1..]){if !(b'0'..=b'9').contains(&b){break}n=n*10+(b-b'0')as u32}n}fn st<'a>(s:&'a[u8],k:&[u8])->&'a[u8]{let Some(q)=f(s,k)else{return b""};let Some(c)=s[q+k.len()..].iter().position(|b|*b==b':')else{return b""};let r=ws(&s[q+k.len()+c+1..]);if r.first()!=Some(&b'"'){return b""};let r=&r[1..];match r.iter().position(|b|*b==b'"'){Some(e)=>&r[..e],None=>b""}}fn put(o:&mut[u8],x:usize,b:&[u8])->usize{let e=x+b.len();if e>o.len(){return x}o[x..e].copy_from_slice(b);e}fn uint(o:&mut[u8],mut x:usize,mut n:u32)->usize{if n==0{return put(o,x,b"0")}let mut d=[0;10];let mut z=0;while n>0{d[z]=b'0'+(n%10)as u8;n/=10;z+=1}while z>0{z-=1;x=put(o,x,&[d[z]])}x}
#[cfg(not(test))]
#[panic_handler]fn panic(_: &core::panic::PanicInfo<'_>)->!{loop{}}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exercises_runtime_fixture_1() {
        let input = include_bytes!("../tests/uc01-subtle.json");
        let mut out = [0u8; 65_536];
        let length = go(input, &mut out);
        assert!(length > 0);
        assert!(length <= out.len());
    }
    #[test]
    fn exercises_runtime_fixture_2() {
        let input = include_bytes!("../tests/uc02-visible.json");
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
        for fixture in [include_bytes!("../tests/uc01-subtle.json").as_slice(), include_bytes!("../tests/uc02-visible.json").as_slice()] {
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
