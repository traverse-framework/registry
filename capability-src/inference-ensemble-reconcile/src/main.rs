#![cfg_attr(not(test), no_std)]
#![cfg_attr(not(test), no_main)]
const N:usize=8192;const M:usize=2048;
#[repr(C)]struct V{buffer:*const u8,length:usize}#[repr(C)]struct VM{buffer:*mut u8,length:usize}
#[link(wasm_import_module="wasi_snapshot_preview1")]unsafe extern "C"{fn fd_read(fd:u32,v:*const VM,c:usize,r:*mut usize)->u32;fn fd_write(fd:u32,v:*const V,c:usize,w:*mut usize)->u32;}
static mut I:[u8;N]=[0;N];static mut O:[u8;M]=[0;M];
#[cfg(not(test))]
#[unsafe(no_mangle)]pub extern "C" fn _start(){unsafe{let p=core::ptr::addr_of_mut!(I).cast::<u8>();let mut n=0;loop{let mut r=0;let v=VM{buffer:p.add(n),length:N-n};if fd_read(0,&v,1,&mut r)!=0||r==0{break}n+=r;if n==N{break}}let o=core::slice::from_raw_parts_mut(core::ptr::addr_of_mut!(O).cast(),M);let z=run(core::slice::from_raw_parts(p,n),o);let mut w=0;let v=V{buffer:core::ptr::addr_of!(O).cast(),length:z};let _=fd_write(1,&v,1,&mut w);}}
fn find(i:&[u8],k:&[u8])->Option<usize>{i.windows(k.len()).position(|w|w==k)}fn int(i:&[u8],k:&[u8])->Option<i32>{let p=find(i,k)?;let c=i[p+k.len()..].iter().position(|b|*b==b':')?;let mut r=&i[p+k.len()+c+1..];while r.first().is_some_and(|b|b.is_ascii_whitespace()){r=&r[1..]}let mut n=0;let mut a=false;for b in r{if !b.is_ascii_digit(){break}n=n*10+(*b-b'0')as i32;a=true}if a{Some(n)}else{None}}
fn string<'a>(i:&'a [u8],k:&[u8])->&'a [u8]{let Some(p)=find(i,k)else{return b""};let Some(c)=i[p+k.len()..].iter().position(|b|*b==b':')else{return b""};let mut r=&i[p+k.len()+c+1..];while r.first().is_some_and(|b|b.is_ascii_whitespace()){r=&r[1..]}if r.first()!=Some(&b'"'){return b""}r=&r[1..];let Some(e)=r.iter().position(|b|*b==b'"')else{return b""};&r[..e]}
fn put(o:&mut[u8],a:&mut usize,b:&[u8]){o[*a..*a+b.len()].copy_from_slice(b);*a+=b.len()}fn num(o:&mut[u8],a:&mut usize,mut n:i32){if n==0{put(o,a,b"0");return}let mut d=[0;12];let mut c=0;while n>0{d[c]=b'0'+(n%10)as u8;n/=10;c+=1}while c>0{c-=1;put(o,a,&d[c..c+1])}}
fn run(i:&[u8],o:&mut[u8])->usize{let id=string(i,b"\"evidence_id\"");let a=int(i,b"\"score_a_millis\"").unwrap_or(-1);let b=int(i,b"\"score_b_millis\"").unwrap_or(-1);let limit=int(i,b"\"max_disagreement_millis\"").unwrap_or(-1);if id.is_empty()||a<0||a>1000||b<0||b>1000||limit<0||limit>1000{return err(o,b"invalid_request")};let diff=if a>b{a-b}else{b-a};let (score,class)=if diff<=limit{((a+b)/2,b"consensus".as_slice())}else{(0,b"disagreement".as_slice())};let mut x=0;put(o,&mut x,b"{\"evidence_id\":\"");put(o,&mut x,id);put(o,&mut x,b"\",\"score_a_millis\":");num(o,&mut x,a);put(o,&mut x,b",\"score_b_millis\":");num(o,&mut x,b);put(o,&mut x,b",\"reconciled_score_millis\":");num(o,&mut x,score);put(o,&mut x,b",\"policy_version\":\"ens-1\",\"result_class\":\"");put(o,&mut x,class);put(o,&mut x,b"\"}");x}
fn err(o:&mut[u8],c:&[u8])->usize{let mut a=0;put(o,&mut a,b"{\"result_class\":\"");put(o,&mut a,c);put(o,&mut a,b"\"}");a}
#[cfg(test)]
mod tests {
    use super::*;

    fn reconcile(input: &[u8]) -> String {
        let mut output = [0u8; M];
        let length = run(input, &mut output);
        String::from_utf8(output[..length].to_vec()).unwrap()
    }

    #[test]
    fn produces_consensus_for_bounded_disagreement() {
        let result = reconcile(br#"{"evidence_id":"e1","score_a_millis":700,"score_b_millis":680,"max_disagreement_millis":25}"#);
        assert!(result.contains("\"reconciled_score_millis\":690"));
        assert!(result.contains("\"result_class\":\"consensus\""));
    }

    #[test]
    fn reports_disagreement_and_handles_equal_scores() {
        let disagreement = reconcile(br#"{"evidence_id":"e2","score_a_millis":900,"score_b_millis":100,"max_disagreement_millis":10}"#);
        assert!(disagreement.contains("\"reconciled_score_millis\":0"));
        assert!(disagreement.contains("\"result_class\":\"disagreement\""));
        let equal = reconcile(br#"{"evidence_id":"e3","score_a_millis":500,"score_b_millis":500,"max_disagreement_millis":0}"#);
        assert!(equal.contains("\"reconciled_score_millis\":500"));
    }

    #[test]
    fn rejects_invalid_inputs_and_parses_fields() {
        for input in [
            br#"{"score_a_millis":1,"score_b_millis":1,"max_disagreement_millis":1}"#.as_slice(),
            br#"{"evidence_id":"x","score_a_millis":1001,"score_b_millis":1,"max_disagreement_millis":1}"#.as_slice(),
            br#"{"evidence_id":"x","score_a_millis":1,"score_b_millis":1,"max_disagreement_millis":1001}"#.as_slice(),
        ] { assert!(reconcile(input).contains("invalid_request")); }
        assert_eq!(int(br#"{"n":-1}"#, b"\"n\""), None);
        assert_eq!(string(br#"{"id":"abc"}"#, b"\"id\""), b"abc");
    }
}
#[cfg(not(test))]
#[cfg(not(test))]
#[panic_handler]fn panic(_: &core::panic::PanicInfo<'_>)->!{loop{}}
