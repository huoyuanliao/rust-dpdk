#[macro_use]
extern crate log;
extern crate env_logger;
extern crate getopts;

extern crate rte;

use std::process::exit;
use std::env;
use std::str::FromStr;
use std::time::Duration;
use std::path::Path;

use rte::eal;
use rte::mbuf;
use rte::ethdev;

const MAX_RX_QUEUE_PER_LCORE: u32 = 16;

const MAX_TIMER_PERIOD: u32 = 86400; /* 1 day max */

const NB_MBUF: u32 = 8192;

// display usage
fn l2fwd_usage(program: &String, opts: getopts::Options) -> ! {
    let brief = format!("Usage: {} [EAL options] -- [options]", program);

    print!("{}", opts.usage(&brief));

    exit(-1);
}

// Parse the argument given in the command line of the application
fn l2fwd_parse_args(args: &Vec<String>) -> (u32, u32, Duration) {
    let mut opts = getopts::Options::new();
    let program = args[0].clone();

    opts.optopt("p",
                "",
                "hexadecimal bitmask of ports to configure",
                "PORTMASK");
    opts.optopt("q",
                "",
                "number of queue (=ports) per lcore (default is 1)",
                "NQ");
    opts.optopt("T",
                "",
                "statistics will be refreshed each PERIOD seconds (0 to disable, 10 default, \
                 86400 maximum)",
                "PERIOD");
    opts.optflag("h", "help", "print this help menu");

    let matches = match opts.parse(&args[1..]) {
        Ok(m) => m,
        Err(err) => {
            println!("Invalid L2FWD arguments, {}", err);

            l2fwd_usage(&program, opts);
        }
    };

    if matches.opt_present("h") {
        l2fwd_usage(&program, opts);
    }

    let mut l2fwd_enabled_port_mask: u32 = 0; /* mask of enabled ports */
    let mut l2fwd_rx_queue_per_lcore: u32 = 1;
    let mut timer_period: u32 = 10;

    if let Some(arg) = matches.opt_str("p") {
        match u32::from_str_radix(arg.as_str(), 16) {
            Ok(mask) if mask != 0 => l2fwd_enabled_port_mask = mask,
            _ => {
                println!("invalid portmask, {}", arg);

                l2fwd_usage(&program, opts);
            }
        }
    }

    if let Some(arg) = matches.opt_str("q") {
        match u32::from_str(arg.as_str()) {
            Ok(n) if 0 < n && n < MAX_RX_QUEUE_PER_LCORE => l2fwd_rx_queue_per_lcore = n,
            _ => {
                println!("invalid queue number, {}", arg);

                l2fwd_usage(&program, opts);
            }
        }
    }

    if let Some(arg) = matches.opt_str("T") {
        match u32::from_str(arg.as_str()) {
            Ok(t) if 0 < t && t < MAX_TIMER_PERIOD => timer_period = t,
            _ => {
                println!("invalid timer period, {}", arg);

                l2fwd_usage(&program, opts);
            }
        }
    }

    (l2fwd_enabled_port_mask, l2fwd_rx_queue_per_lcore, Duration::from_secs(timer_period as u64))
}

fn main() {
    env_logger::init().unwrap();

    let mut args: Vec<String> = env::args().collect();
    let program = String::from(Path::new(&args[0]).file_name().unwrap().to_str().unwrap());

    let (eal_args, opt_args) = if let Some(pos) = args.iter().position(|arg| arg == "--") {
        let (eal_args, opt_args) = args.split_at_mut(pos);

        opt_args[0] = program;

        (eal_args.to_vec(), opt_args.to_vec())
    } else {
        (args[..1].to_vec(), args.clone())
    };

    debug!("eal args: {:?}, l2fwd args: {:?}", eal_args, opt_args);

    let (l2fwd_enabled_port_mask, l2fwd_rx_queue_per_lcore, timer_period) =
        l2fwd_parse_args(&opt_args);

    // init EAL
    eal::init(&eal_args);

    // create the mbuf pool
    let l2fwd_pktmbuf_pool = mbuf::pktmbuf_pool_create("mbuf_pool",
                                                       NB_MBUF,
                                                       32,
                                                       0,
                                                       mbuf::RTE_MBUF_DEFAULT_BUF_SIZE,
                                                       eal::socket_id())
                                 .expect("Cannot init mbuf pool");

    let mut nb_ports = ethdev::count();

    if nb_ports == 0 {
        println!("No Ethernet ports - bye");

        exit(0);
    }

    if nb_ports > rte::RTE_MAX_ETHPORTS {
        nb_ports = rte::RTE_MAX_ETHPORTS;
    }

    // list of enabled ports
    let mut l2fwd_dst_ports = [0u8; rte::RTE_MAX_ETHPORTS as usize];
    let mut last_port = 0;
    let mut nb_ports_in_mask = 0;

    // Each logical core is assigned a dedicated TX queue on each port.
    for port_id in 0..nb_ports as u8 {
        // skip ports that are not enabled
        if (l2fwd_enabled_port_mask & (1 << port_id)) == 0 {
            continue;
        }

        if (nb_ports_in_mask % 2) != 0 {
            l2fwd_dst_ports[port_id as usize] = last_port;
            l2fwd_dst_ports[last_port as usize] = port_id;
        } else {
            last_port = port_id;
        }

        nb_ports_in_mask += 1;

        let info = ethdev::dev_info(port_id);

        debug!("port #{} -> {:?}", port_id, info);
    }
}
