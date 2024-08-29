// println!("Hello, world!");

/*
curl -sS -X GET -G "https://forum.dfinity.org/search.json" --data-urlencode 'q=after:2020-01-01' --data-urlencode 'page=1'

で対象のtopic_idを全て取得して、それぞれに対してrawのtextを取り出す。
rawはpageごとに分かれているので、それらを別々のファイルとして保存する。
https://forum.dfinity.org/raw/16643?page=2

基本的に増える方向なので、ファイルは全て更新されるが、万が一、そのthreadのpost数が減ると、pageも減ってしまう。
対象のディレクトリのファイルを全て消して、再度、pageをファイルとして全て書き込む。

refresh-optionで、すでにファイルがあるときに削除してやり直すか、続きからやるか。
基本動作は削除するので、続きからやるモードは初期化時の一度しか使わない。

pageを全て取り出した時のみにファイルとして書き込むようにすると、ファイルが存在する->不完全ではない。

search_after_2020
*/

use anyhow::Result;
use core::panic;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet}, fmt::format, fs::File, io::{Read, Write}, path::Path, process, sync::{Arc, Mutex}, thread::sleep, time::Duration
};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        // #[arg(long)]
        // ic: bool,

        // #[arg(long, default_value = "default")]
        // name: String,

        // #[arg(long, default_value = "1024")]
        // chunk_kib_size: usize,

        // source_data_path: String,
        // graph_metadata_path: String,
        // target_canister_id: String,
    },
    Raw {
        // #[arg(long)]
        // ic: bool,
        // #[arg(long)]
        // simd: bool,
        // #[arg(long, default_value = "./query_set/query.public.10K.fbin")]
        // query_path: String,
        // #[arg(long, default_value = "./query_set/gt/deep100M_groundtruth.ivecs")]
        // ground_truth_path: String,

        // target_canister_id: String,
    },

    SpecificRaw {},
    Update {},
}

#[tokio::main]
async fn main() -> Result<()> {
    let origin = "https://forum.dfinity.org";
    let search_range = ("2019-10-01", "2024-08-19");
    let error_log_file_name = format!(
        "error_after_{}_before_{}.json",
        search_range.0, search_range.1
    );

    let cli = Cli::parse();

    match cli.command {
        Commands::Init {} => {
            let max_id = 34306;
            // let page = Arc::new(Mutex::new(1u64));
            let id_set = Arc::new(Mutex::new(HashSet::new()));
            let current_id = Arc::new(Mutex::new(1));

            // Arc for ctrlc handler
            let current_id_clone = Arc::clone(&current_id);
            let id_set_clone = Arc::clone(&id_set);
            let error_log_file_name_clone = error_log_file_name.clone();

            ctrlc::set_handler(move || {
                println!("Ctrl-C detected! Cleaning up and exiting...");

                let current_id = *current_id_clone.lock().unwrap();
                let id_set = id_set_clone.lock().unwrap();

                let json = serde_json::to_string(&json!({
                    "fail_id": current_id,
                    "id_set": *id_set,
                    "err": "Ctrl-C detected! Cleaning up and exiting..."
                }))
                .unwrap();
                let mut file = File::create(&error_log_file_name_clone).unwrap();
                file.write_all(json.as_bytes()).unwrap();

                // Err(anyhow::anyhow!(serde_json::to_string(&json!(map)).unwrap()))

                process::exit(0); // 正常終了
            })
            .expect("Error setting Ctrl-C handler");

            match File::open(error_log_file_name.clone()) {
                Ok(mut file) => {
                    println!("continue form error file");
                    let mut contents = String::new();
                    file.read_to_string(&mut contents)?;
                    let mut json: HashMap<String, Value> = serde_json::from_str(&contents)?;

                    let Value::Array(_id_set) = json.remove("id_set").unwrap() else {
                        panic!()
                    };
                    println!("id_set len: {}", _id_set.len());
                    *id_set.lock().unwrap() = _id_set
                        .into_iter()
                        .map(|v| {
                            let Value::Number(id) = v else { panic!() };
                            id.as_u64().unwrap()
                        })
                        .collect();

                    let Value::Number(fail_id) = json.remove("fail_id").unwrap() else {
                        panic!()
                    };
                    *current_id.lock().unwrap() = fail_id.as_u64().unwrap();
                }
                Err(_) => {
                    println!("no error file")
                }
            }

            while *current_id.lock().unwrap() <= max_id {
                let topic_status_url = format!("{origin}/t/{}.json", current_id.lock().unwrap());

                match reqwest::Client::new()
                    .get(&topic_status_url)
                    // .query(&[("q", &search_range_query), ("page", &format!("{}", *page.lock().unwrap()))])
                    .send()
                    .await
                {
                    Ok(response) => {
                        let mut map = response.json::<HashMap<String, Value>>().await?;

                        if let Some(error_type) = map.remove("error_type") {
                            println!(
                                "id={}, error_type {}",
                                *current_id.lock().unwrap(),
                                error_type
                            );
                        } else {
                            id_set.lock().unwrap().insert(*current_id.lock().unwrap());
                            let Some(Value::String(created_at)) = map.remove("created_at") else {
                                println!("{:?}", map);
                                break;
                            };
                            let Some(Value::String(title)) = map.remove("title") else {
                                println!("{:?}", map);
                                break;
                            };
                            println!(
                                "id={}, created_at {}, id_set len: {}, title: [{}]",
                                *current_id.lock().unwrap(),
                                created_at,
                                id_set.lock().unwrap().len(),
                                title
                            );
                        }

                        sleep(Duration::from_millis(100));
                    }
                    Err(e) => {
                        println!("{e}");

                        break;
                    }
                };

                *current_id.lock().unwrap() += 1;
            }

            println!("stop");

            // let json = serde_json::to_string(&json!(*id_set.lock().unwrap())).unwrap();
            // let mut file = File::create(id_set_file_name)?;
            // file.write_all(json.as_bytes())?;
            let json = serde_json::to_string(&json!({"fail_id":  *current_id.lock().unwrap(), "id_set": *id_set.lock().unwrap(), "err": "id reached max"})).unwrap();
            let mut file = File::create(error_log_file_name.clone())?;
            file.write_all(json.as_bytes())?;
        }
        Commands::Raw {} => {
            let mut contents = String::new();
            File::open(error_log_file_name.clone())?.read_to_string(&mut contents)?;
            let mut json: HashMap<String, Value> = serde_json::from_str(&contents)?;

            let Some(Value::Array(id_set)) = json.remove("id_set") else {
                panic!()
            };

            let topic_ids: Vec<_> = id_set.into_iter().map(|value| {
                if let Value::Number(id) = value {id.as_u64().unwrap()} else {panic!()}
            }).collect();

            let topics_len =  topic_ids.len();
            println!("topics len: {}", topics_len);


            for (count, topic_id) in topic_ids.into_iter().enumerate() {
                
                // If already the file exsits
                let topic_dir = format!("documents/topic_{topic_id}");
                if Path::new(&topic_dir).is_dir() {
                    continue;
                }

                let raw_text_url = format!("{origin}/raw/{topic_id}");
                let mut pages = Vec::new();
                let mut page_number = 1;

                // Loop until respoinse content is empty
                loop {
                    let response_text = reqwest::Client::new()
                        .get(&raw_text_url)
                        .query(&[("page", page_number.to_string())])
                        .send()
                        .await?
                        .text()
                        .await?;

                    if response_text.len() == 0 {
                        break;
                    } else {
                        pages.push(response_text)
                    }

                    println!("loaded raw page={page_number}");

                    page_number += 1;
                    sleep(Duration::from_millis(100));
                }


                let tmp_dir = "documents/topic_tmp";
                if Path::new(&tmp_dir).exists() {
                    std::fs::remove_dir_all(&tmp_dir)?;
                }
                std::fs::create_dir_all(&tmp_dir)?;
                for (page_number, page_text) in pages.into_iter().enumerate() {
                    let mut file = File::create(format!("{tmp_dir}/page_{page_number}.txt"))?;
                    file.write_all(page_text.as_bytes())?;
                }
                std::fs::rename(tmp_dir, topic_dir)?;

                println!("{}/{topics_len}: id={topic_id}\n", count+1)


                // 
                // std::fs::create_dir_all(topic_dir)?
            }
            

        }
        Commands::SpecificRaw {} => {
            let mut contents = String::new();
            // File::open(error_log_file_name.clone())?.read_to_string(&mut contents)?;
            // let mut json: HashMap<String, Value> = serde_json::from_str(&contents)?;

            // let Some(Value::Array(id_set)) = json.remove("id_set") else {
            //     panic!()
            // };

            // let topic_ids: Vec<_> = id_set.into_iter().map(|value| {
            //     if let Value::Number(id) = value {id.as_u64().unwrap()} else {panic!()}
            // }).collect();

            // 7, 18, 19, 20

            let topics_len =  topic_ids.len();
            println!("topics len: {}", topics_len);


            for (count, topic_id) in topic_ids.into_iter().enumerate() {
                
                // If already the file exsits
                let topic_dir = format!("documents/topic_{topic_id}");
                if Path::new(&topic_dir).is_dir() {
                    continue;
                }

                let raw_text_url = format!("{origin}/raw/{topic_id}");
                let mut pages = Vec::new();
                let mut page_number = 1;

                // Loop until respoinse content is empty
                loop {
                    let response_text = reqwest::Client::new()
                        .get(&raw_text_url)
                        .query(&[("page", page_number.to_string())])
                        .send()
                        .await?
                        .text()
                        .await?;

                    if response_text.len() == 0 {
                        break;
                    } else {
                        pages.push(response_text)
                    }

                    println!("loaded raw page={page_number}");

                    page_number += 1;
                    sleep(Duration::from_millis(100));
                }


                let tmp_dir = "documents/topic_tmp";
                if Path::new(&tmp_dir).exists() {
                    std::fs::remove_dir_all(&tmp_dir)?;
                }
                std::fs::create_dir_all(&tmp_dir)?;
                for (page_number, page_text) in pages.into_iter().enumerate() {
                    let mut file = File::create(format!("{tmp_dir}/page_{page_number}.txt"))?;
                    file.write_all(page_text.as_bytes())?;
                }
                std::fs::rename(tmp_dir, topic_dir)?;

                println!("{}/{topics_len}: id={topic_id}\n", count+1)


                // 
                // std::fs::create_dir_all(topic_dir)?
            }
            

        }
        Commands::Update {} => {
            let search_api_url = format!("{origin}/search.json");
            let search_range_query = format!("after:{} before:{}", search_range.0, search_range.1);
        }
    }

    todo!();

    /*
    Todo:
    search_rangeの範囲のidのjsonがすでにあるかチェック。なければ下を実行。あれば、rawを実行。

    */

    #[cfg(feature = "raw")]
    {
        // let id_set = [33851,34237,32670,31230,25069,34057,33540,34230,33611,33478,32724,33829,31494,33329,33802,33333,33657,33821,33464,33797,33344,33470,33874,31109,33661,19872,34056,34021,33660,16560,33367,10562,33589,34134,33679,30255,33899,33547,28147,33550,27360,33843,9424,33656,34149,14543,30890,28007,33384,33318,32337,33573,33725,33944,33290,33934,33717,18583,33472,33718,33980,33756,33875,33523,33816,34096,14680,33665,33924,34121,29048,25967,33622,14682,33716,33767,33591,34232,33564,34145,33947,34161,33761,33837,20369,31597,33345,34220,33372,34010,33416,33566,33731,33328,33651,33531,33818,33896,34034,33799,34136,33421,34212,33606,34148,19895,17606,33614,32686,33360,33645,32996,23938,34137,34027,34080,33878,23551,34077,33819,33476,6156,34228,34084,34150,34013,31821,33288,21828,32840,33279,27395,32905,30170,32823,33410,32993,33889,33407,34036,34246,33401,33211,23553,34125,33040,31783,33712,18893,34040,31840,33999,33173,29128,33772,29583,33346,34032,17089,33370,32458,34235,34168,33429,33881,31277,33337,24966,34222,34180,33994,9403,33471,33440,27917,17677,33460,33684,33753,32502,33548,22059,33775,33833,33977,34047,28141,32610,34058,34250,32975,33565,33197,23560,33659,34039,33473,29486,33544,20836,25308,33911,32977,33579,33623,33770,32692,33655,34092,33296,33424,33814,33852,33780,34050,33275,32922,33037,32802,33378,33721,12103,20316,33870,33941,32794,11925,34118,33438,33422,33596,33647,34178,33612,34192,34090,14265,33762,32222,33072,22819,33462,33613,34238,27907,16566,33892,32258,34187,34015,33752,34071,31837,33580,23810,34035,32721,33856,33962,33430,33361,33666,33832,24621,23951,33496,33982,33545,33872,33897,33876,33754,33895,27700,33281,31187,34216,34067,33259,33108,6468,34114,34194,33635,33778,33535,34055,34223,33757,25274,16643,22597,33916,34014,33859,33391,33902,33533,34105,33629,34091,34163,33686,33287,33835,19848,34185,33305,26384,6152,33663,32343,33289,34195,33839,33640,2190,33664,14463,33685,31693,12109,25672,33243,33863,32125,33928,33567,33303,34116,34016,12888,23893,33504,33653,33302,34219,33764,33643,34119,23872,33880,32837,28315,33793,33352,34104,33353,33740,34124,33862,33783,32734,26812,11941,33728,33690,24832,33888,33437,33981,33893,34229,33668,20969,34026,33236,33366,33483,33605,34204,24259,33442,34221,34256,33468,34169,33845,33741,33536,34231,34258,33883,34133,33993,33301,34120,33518,33293,24803,33694,33426,33507,33869,33502,29975,33788,32431,33857,33268,9747,31534,31803,34213,33954,34189,34024,33299,31274,34190,12700,33532,27920,26128,33965,33627,33423,33331,34019,33755,29180,28993,33885,33765,33971,33467,33489,34142,34208,33326,33569,33758,33377,34263,33978,34170,33639,33674,10473,34052,34215,33624,34106,27592,33148,33670,15562,28365,33855,33201,27718,33616,33278,32687,33926,33562,33823,33956,33744,17275,33763,33759,18919,33850,33308,23408,33610,28181,34076,33715,32941,18821,34209,34254,34107,31865,34210,34255,33593,33309,33600,32930,33644,33662,32238,33681,33722,34051,33215,9395,29274,33526,11781,24479,34066,30886,33405,31784,25110,27633,33711];
        let id_set: Vec<u64> = (0..100).collect();
        let id_len = id_set.len();
        for (i, id) in id_set.into_iter().enumerate() {
            // todo: idのフォルダがあればスキップ

            if Path::new(format!("topic_{id}")).is_dir() {
                continue;
            }

            let raw_text_url = format!("{origin}/raw/{id}");
            let mut page = 1;

            let mut pages = Vec::new();

            loop {
                let response_text = reqwest::Client::new()
                    .get(&raw_text_url)
                    .query(&[("page", page.to_string())])
                    .send()
                    .await?
                    .text()
                    .await?;

                println!("\n\nresponse_text\n{}", response_text);

                if response_text.len() == 0 {
                    println!("end page");
                    break;
                } else {
                    pages.push(response_text)
                }

                page += 1;
                sleep(Duration::from_secs(1));
            }

            // wip save the files
            // tmp dirに書き込んで、それらを一気に移動させる。

            println!("{i}/{id_len}");

            // println!("\n\n========================================================\n{response_text}");
        }
    }

    #[cfg(feature = "latest")]
    let id_set = {
        // let mut page = Arc::new(1);
        // let mut id_set = HashSet::new();
        let page = Arc::new(Mutex::new(1u64));
        let id_set = Arc::new(Mutex::new(HashSet::new()));

        // Arc for ctrlc handler
        let page_clone = Arc::clone(&page);
        let id_set_clone = Arc::clone(&id_set);
        let error_log_file_name_clone = error_log_file_name.clone();

        ctrlc::set_handler(move || {
            println!("Ctrl-C detected! Cleaning up and exiting...");

            let page = *page_clone.lock().unwrap();
            let id_set = id_set_clone.lock().unwrap();

            let json = serde_json::to_string(&json!({
                "fail_page": page,
                "id_set": *id_set,
                "err": "Ctrl-C detected! Cleaning up and exiting..."
            }))
            .unwrap();
            let mut file = File::create(&error_log_file_name_clone).unwrap();
            file.write_all(json.as_bytes()).unwrap();

            // Err(anyhow::anyhow!(serde_json::to_string(&json!(map)).unwrap()))

            process::exit(0); // 正常終了
        })
        .expect("Error setting Ctrl-C handler");

        match File::open(error_log_file_name.clone()) {
            Ok(mut file) => {
                println!("continue form error file");
                let mut contents = String::new();
                file.read_to_string(&mut contents)?;
                let mut json: HashMap<String, Value> = serde_json::from_str(&contents)?;

                let Value::Array(_id_set) = json.remove("id_set").unwrap() else {
                    panic!()
                };
                println!("id_set len: {}", _id_set.len());
                *id_set.lock().unwrap() = _id_set
                    .into_iter()
                    .map(|v| {
                        let Value::Number(id) = v else { panic!() };
                        id.as_u64().unwrap()
                    })
                    .collect();

                let Value::Number(_page) = json.remove("fail_page").unwrap() else {
                    panic!()
                };
                *page.lock().unwrap() = _page.as_u64().unwrap();
            }
            Err(_) => {
                println!("no error file")
            }
        }

        loop {
            let topics_len = match reqwest::Client::new()
                .get(&search_api_url)
                .query(&[
                    ("q", &search_range_query),
                    ("page", &format!("{}", *page.lock().unwrap())),
                ])
                .send()
                .await
            {
                Ok(response) => {
                    let mut map = response.json::<HashMap<String, Value>>().await?;

                    let Some(Value::Array(topics)) = map.remove("topics") else {
                        println!("not topics key");
                        let json = serde_json::to_string(&json!({"fail_page":  *page.lock().unwrap(), "id_set": *id_set.lock().unwrap(), "err": map})).unwrap();
                        let mut file = File::create(error_log_file_name.clone())?;
                        file.write_all(json.as_bytes())?;

                        Err(anyhow::anyhow!(serde_json::to_string(&json!(map)).unwrap()))?
                    };

                    let topics_len = topics.len();

                    let ids: Vec<u64> = topics
                        .iter()
                        .map(|topic| {
                            let Value::Object(mut topic) = topic.clone() else {
                                panic!()
                            };

                            let Value::Number(id) = topic.remove("id").unwrap() else {
                                panic!("id is not a Number type value")
                            };
                            let id = id.as_u64().expect("");
                            id
                        })
                        .collect();
                    println!("\n\n{:?}", ids);

                    topics.into_iter().for_each(|topic| {
                        let Value::Object(mut topic) = topic else {
                            panic!()
                        };

                        let Value::Number(id) = topic.remove("id").unwrap() else {
                            panic!("id is not a Number type value")
                        };
                        let id = id.as_u64().expect("");
                        // println!("id: {id}");
                        id_set.lock().unwrap().insert(id);
                    });

                    let Some(Value::Object(grouped_search_result)) =
                        map.get("grouped_search_result")
                    else {
                        println!("not exsit grouped_search_result");
                        break;
                    };
                    let Some(Value::Bool(more_full_page_results)) =
                        grouped_search_result.get("more_full_page_results")
                    else {
                        println!("not exsit more_full_page_results");
                        break;
                    };
                    if !more_full_page_results {
                        println!("more_full_page_results is false");
                        break;
                    }

                    *page.lock().unwrap() += 1;
                    // sleep(Duration::from_secs(2));

                    println!(
                        "page={}, topics len: {}, id_set len: {}",
                        *page.lock().unwrap() - 1,
                        topics_len,
                        id_set.lock().unwrap().len()
                    );

                    if *page.lock().unwrap() % 16 == 0 {
                        println!("waiting 1 min...");
                        sleep(Duration::from_secs(61));
                    };

                    topics_len
                }
                Err(e) => {
                    let json = serde_json::to_string(&json!({"fail_page":  *page.lock().unwrap(), "id_set": *id_set.lock().unwrap(), "err": e.to_string()})).unwrap();
                    let mut file = File::create(error_log_file_name.clone())?;
                    file.write_all(json.as_bytes())?;

                    println!("{e}");
                    Err(e)?
                }
            };
        }

        println!("finish");

        let json = serde_json::to_string(&json!(*id_set.lock().unwrap())).unwrap();
        let mut file = File::create(id_set_file_name)?;
        file.write_all(json.as_bytes())?;

        id_set
    };

    Ok(())

    // todo!();

    // {
    //     let id_len = id_set.len();
    //     for (i, id) in id_set.into_iter().enumerate() {
    //         // todo: idのフォルダがあればスキップ

    //         let raw_text_url = format!("{origin}/raw/{id}");
    //         let mut page = 1;

    //         loop {
    //             let response_text = reqwest::Client::new()
    //                 .get(&raw_text_url)
    //                 .query(&[("page", page.to_string())])
    //                 .send()
    //                 .await?
    //                 .text()
    //                 .await?;

    //             if response_text.len() == 0 {
    //                 println!("end page");
    //                 break;
    //             }

    //             page += 1;
    //             sleep(Duration::from_secs(1));
    //         }

    //         println!("{i}/{id_len}");

    //         // println!("\n\n========================================================\n{response_text}");
    //     }
    // }
}
