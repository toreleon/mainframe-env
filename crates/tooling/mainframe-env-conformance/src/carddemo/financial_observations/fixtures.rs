//! Immutable hashes of complete independent pinned-source expectations.
//! Raw29/3 fixtures and transparent arithmetic producer remain outside Git.
//! Oracle SHA256: 6a29aa133a1e0ec73805cb4f9d1fb7c3e26f9c5c0017efb3446d627cb0ccd74d
//! Raw fixture seal SHA256: a9da8ee9664ce24821dd085f840a3bd1275dd91617376c9986b61d5f335a0922
//! PRTCATBL negative formatting is expressly local; zero IBM format credit.
//! Spool bytes keep the independent source oracle; DD selectors use the LOCAL
//! ProgramOutput.records issuing-step SYSPRINT carrier. No IBM DISPLAY routing credit.
use super::{DatasetExpected, SpoolExpected};

pub(super) const CORPUS_COMMIT: &str = "59cc6c2fd7ebd7ef7925cad552a01a4b8b6e4d5e";
pub(super) const CORRECTED_STATEMENT_SHA256: &str =
    "cb848cd5a03c4cefc9b8c0f67e0c6b1248af6c7f000c405bf011b47d8f3f17a8";
pub(super) const SOURCE_PINS: &[(&str, &str)] = &[
    (
        "app/cbl/CBTRN02C.cbl",
        "708f3cadc555acab63f11e2f3238f5372ac7180e6b01197bf960d96bf0d2e83f",
    ),
    (
        "app/cbl/CBACT04C.cbl",
        "5084bb8b0c9a0f0199f737487ae1863f12e43cabbdc459a62b6b67bedc683cc4",
    ),
    (
        "app/cbl/CBTRN03C.cbl",
        "8691e625502b7efcfd7ba4ae61797132e7b68f3fd5fedcdcc7dd6488f6871eef",
    ),
    (
        "app/cbl/CBSTM03A.CBL",
        "23c8753b6b4e0c24d4560c83861fe8162626bab195faec0fe88cf80b8bf432b5",
    ),
    (
        "app/cbl/CBSTM03B.CBL",
        "ac004f7f40dcb3f2a14dfb83a743f83fed9383675bc93452f3424c40cb6475a7",
    ),
    (
        "app/cpy/CVACT01Y.cpy",
        "81a08bad15af5664326a6f0af3650f570821c4857ffdec3a6a39f91f07dca728",
    ),
    (
        "app/cpy/CVACT03Y.cpy",
        "ffc6079e09b28739e154bf6c1e1c36d408209faa91f6cf7008078dc596a1c370",
    ),
    (
        "app/cpy/CVTRA01Y.cpy",
        "50637f13692c89b17a2fc60d249dc54e9eb3569d933afca3cdcf65b491a9d5ba",
    ),
    (
        "app/cpy/CVTRA02Y.cpy",
        "7828fae489c59944b4310e223028a8d3a525ccf4ead113c062bcaff04b52bf9c",
    ),
    (
        "app/cpy/CVTRA03Y.cpy",
        "fb15dbc4a6924cbddce0704932f9e61c067ac10004cd00839dff0ed7c7bb0667",
    ),
    (
        "app/cpy/CVTRA04Y.cpy",
        "89803bc13a06347e1f3d8a599a5383b87235b203dce9f18a486bc233186479f8",
    ),
    (
        "app/cpy/CVTRA05Y.cpy",
        "d7bde0e78ff608497087b9909c889ed39e347269f964bda767b92b547fbb5fec",
    ),
    (
        "app/cpy/CVTRA06Y.cpy",
        "c5c69f1b86c5a10156d3c5881d7cf387e6b925aae32825360f85bf4056a554a1",
    ),
    (
        "app/cpy/CVTRA07Y.cpy",
        "72ba597b1a40e1e6cf908e15da9e6a818a0ab899ef1d27d15edeb074963102fa",
    ),
    (
        "app/cpy/COSTM01.CPY",
        "108170f314aa951281fdda321c4609c89638caee8d4d7085d08cdca5dc6bec36",
    ),
    (
        "app/cpy/CUSTREC.cpy",
        "f87a0a041fbd8f38dc9c23a9519b917af9c7008527e9cb924f91c2fb4e0a2ecc",
    ),
    (
        "app/jcl/POSTTRAN.jcl",
        "ecff62c691e6ce101de08690e72ec065bc98bd845744ddf914899097d37c9191",
    ),
    (
        "app/jcl/INTCALC.jcl",
        "61afa664a807558e58213641d9f3317ab3b354a350c4c1536a630897d194d275",
    ),
    (
        "app/jcl/TRANBKP.jcl",
        "457cd00d14a1d9ac9983d92212541df3456e8428862b97cf10bab7249b9e6183",
    ),
    (
        "app/jcl/COMBTRAN.jcl",
        "ab60da6cfdc8c4ec66c8b950540553bf35e91dc1e61a1dd4ada5bb888011c85c",
    ),
    (
        "app/jcl/TRANIDX.jcl",
        "64a06b44c816ee666a0e82e24fbf040d5ed792d158c93a0ac477db5dfa30dcae",
    ),
    (
        "app/jcl/TRANREPT.jcl",
        "7d8fc0777e6b9fb1c62aee6b4b10a67d127057c84b92203c7152f230b3db9571",
    ),
    (
        "app/jcl/PRTCATBL.jcl",
        "24ba68991b615b24b642ec9365b7a06376f8a813368f6656fcbbb2f4fb2031a2",
    ),
    (
        "app/jcl/CREASTMT.JCL",
        "d32627022d7686d1811ba253faa2c5d86295c8055428b2a610a27d8f84751950",
    ),
    (
        "app/proc/REPROC.prc",
        "4562039d1c7b90f05cc946dd1cd1cb93571bb279740c5c9777acfa3ea23fbc9f",
    ),
    (
        "app/ctl/REPROCT.ctl",
        "5c6b8f26aa53c23131265e715b53130a1a7afd88b5bd3319aeca91f3d1a55bf5",
    ),
    (
        "app/jcl/TRANFILE.jcl",
        "360080ae5bf5153c3ecbf82385b070a6c7a2cdbf18870685951bf9894516730a",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.ACCTDATA.PS",
        "23167cdff65ca6dfa2e5bccee89112e3e83a80247a38e251f3981377ab098ec9",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.TCATBALF.PS",
        "725dbe47d34d67053090efde8819af686ce424ed9dc0c8fd3f33cd1bf786b68a",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CARDXREF.PS",
        "b07ab2e5fa2500501c1dcaffe478ee74c4562a290174020a093dfa6f0e6325d3",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.DISCGRP.PS",
        "722df78926c1ac01ed8e1138d2bba72593ac21046c994685c64fc7c87a5851d3",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS",
        "479b1f99cb7adcd9b79e94708f04c8bde0a010ba87f2ed69ba8af1effe57d076",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.DALYTRAN.PS.INIT",
        "aef36e51163a80999656487416e9edeec70b1e190fc70185c8e86c577e135073",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.TRANTYPE.PS",
        "97735eb6979387143a98947a0681aed745a63c6a1f7dfbea1667dd4a6d567cf2",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.TRANCATG.PS",
        "3c171c53c0b90ec7b0c267305cf5ba051825dfff78021425c601141f2f3c99e2",
    ),
    (
        "app/data/EBCDIC/AWS.M2.CARDDEMO.CUSTDATA.PS",
        "0435915cd35ea3077bad9d391fea1520bc7f30ec06d6e29fbe68290f3d6f27f3",
    ),
];

pub(super) const DATASETS: &[DatasetExpected] = &[
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        width: 300,
        records: 50,
        key: Some((0, 11)),
        framed_sha256: "d7d016c720e90f52dc84832ca1c29387a0707403b42f66542baea85f576f3ce4",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
        width: 50,
        records: 50,
        key: Some((0, 17)),
        framed_sha256: "e9efd399e24c7625809f81dba773fc20488ddb325e62f4de7c8c5a594376d056",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        width: 350,
        records: 1,
        key: Some((0, 16)),
        framed_sha256: "c2018baf795432bf9d3cf167ed8f882cc0d311ee5b89278e3698922d751be148",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
        width: 50,
        records: 50,
        key: Some((0, 16)),
        framed_sha256: "ee8f7c0cd7ce5ca3b6cf5e6138d0f5ea24eae84b03ac074c9893fb31e85009dc",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.DISCGRP.VSAM.KSDS",
        width: 50,
        records: 51,
        key: Some((0, 16)),
        framed_sha256: "4d6d9fbf63afab20956545aaeaf530cbd3afa41b69523cea87389a931bfa737e",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.DALYTRAN.PS",
        width: 350,
        records: 300,
        key: None,
        framed_sha256: "d4089849270a60dcbd21b55df1469449027b7d1a0d39d8a41f2acf1469031bd9",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS",
        width: 60,
        records: 7,
        key: Some((0, 2)),
        framed_sha256: "5f1df26769299fd0374ac2f84efa5e7f31617f459285e214f6b87c02d9e5a367",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS",
        width: 60,
        records: 18,
        key: Some((0, 6)),
        framed_sha256: "7bbcc0f1bad712606239c53a7cce1934b4f123957de203b3d35b808e8fa68e77",
    },
    DatasetExpected {
        phase: "initial",
        name: "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
        width: 500,
        records: 50,
        key: Some((0, 9)),
        framed_sha256: "615f8a1092eb007386e77857f1d0ab793e9c32a708014a8391747cf73c554209",
    },
    DatasetExpected {
        phase: "POSTTRAN",
        name: "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        width: 300,
        records: 50,
        key: Some((0, 11)),
        framed_sha256: "b324e2e6441728046b544b86d9804aa52e1d0da24c487b0ee059885681807bf1",
    },
    DatasetExpected {
        phase: "POSTTRAN",
        name: "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
        width: 50,
        records: 100,
        key: Some((0, 17)),
        framed_sha256: "67f5644e79a0ca139671c497171b7994a943cbdca948f0b39fe0de7c37dff530",
    },
    DatasetExpected {
        phase: "POSTTRAN",
        name: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        width: 350,
        records: 262,
        key: Some((0, 16)),
        framed_sha256: "c5b97c281f8260af664fa7290cbe67421fa543ce34a2c479d5d10f061fbfbbe3",
    },
    DatasetExpected {
        phase: "POSTTRAN",
        name: "AWS.M2.CARDDEMO.DALYREJS.G0001V00",
        width: 430,
        records: 38,
        key: None,
        framed_sha256: "48cedf1c2dd27dae65d072f20e19a74c0101d96f539d0e018a2b1541456336a4",
    },
    DatasetExpected {
        phase: "INTCALC",
        name: "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
        width: 300,
        records: 50,
        key: Some((0, 11)),
        framed_sha256: "522e51c38ab2934a2c2278bee613124add5547b1f3bd49204fdceb3b2f69b164",
    },
    DatasetExpected {
        phase: "INTCALC",
        name: "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
        width: 50,
        records: 100,
        key: Some((0, 17)),
        framed_sha256: "67f5644e79a0ca139671c497171b7994a943cbdca948f0b39fe0de7c37dff530",
    },
    DatasetExpected {
        phase: "INTCALC",
        name: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        width: 350,
        records: 262,
        key: Some((0, 16)),
        framed_sha256: "c5b97c281f8260af664fa7290cbe67421fa543ce34a2c479d5d10f061fbfbbe3",
    },
    DatasetExpected {
        phase: "INTCALC",
        name: "AWS.M2.CARDDEMO.SYSTRAN.G0001V00",
        width: 350,
        records: 50,
        key: None,
        framed_sha256: "660fbd1999017bffb6e28f1ff4a02fab95661041fdd400417be3f2b85e047b61",
    },
    DatasetExpected {
        phase: "TRANBKP",
        name: "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0001V00",
        width: 350,
        records: 262,
        key: None,
        framed_sha256: "c5b97c281f8260af664fa7290cbe67421fa543ce34a2c479d5d10f061fbfbbe3",
    },
    DatasetExpected {
        phase: "COMBTRAN",
        name: "AWS.M2.CARDDEMO.TRANSACT.COMBINED.G0001V00",
        width: 350,
        records: 312,
        key: None,
        framed_sha256: "11e2d920f7b134117153abd70faec209887ac512aa437aea8c64d4c6891491cc",
    },
    DatasetExpected {
        phase: "COMBTRAN",
        name: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        width: 350,
        records: 312,
        key: Some((0, 16)),
        framed_sha256: "11e2d920f7b134117153abd70faec209887ac512aa437aea8c64d4c6891491cc",
    },
    DatasetExpected {
        phase: "TRANREPT",
        name: "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0002V00",
        width: 350,
        records: 312,
        key: None,
        framed_sha256: "11e2d920f7b134117153abd70faec209887ac512aa437aea8c64d4c6891491cc",
    },
    DatasetExpected {
        phase: "TRANREPT",
        name: "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00",
        width: 350,
        records: 312,
        key: None,
        framed_sha256: "68216ae9d99e5c174915143c1e19f55d65db21ba628e0d4cf24ba41606595927",
    },
    DatasetExpected {
        phase: "TRANREPT",
        name: "AWS.M2.CARDDEMO.TRANREPT.G0001V00",
        width: 133,
        records: 519,
        key: None,
        framed_sha256: "6082f721f3856c8c708e41100ae30b527e17d9587dd453ff82c5df4848a9d525",
    },
    DatasetExpected {
        phase: "PRTCATBL",
        name: "AWS.M2.CARDDEMO.TCATBALF.BKUP.G0001V00",
        width: 50,
        records: 100,
        key: None,
        framed_sha256: "67f5644e79a0ca139671c497171b7994a943cbdca948f0b39fe0de7c37dff530",
    },
    DatasetExpected {
        phase: "PRTCATBL",
        name: "AWS.M2.CARDDEMO.TCATBALF.REPT",
        width: 40,
        records: 100,
        key: None,
        framed_sha256: "d24c5cdb14021a13911f61c9587d9ff08982cb79b9456b03dcd9e4efd409731e",
    },
    DatasetExpected {
        phase: "CREASTMT",
        name: "AWS.M2.CARDDEMO.TRXFL.SEQ",
        width: 350,
        records: 312,
        key: None,
        framed_sha256: "c4801b75007386670b848bd858615dc07feedeb4527c052d8085312912a19054",
    },
    DatasetExpected {
        phase: "CREASTMT",
        name: "AWS.M2.CARDDEMO.TRXFL.VSAM.KSDS",
        width: 350,
        records: 312,
        key: Some((0, 32)),
        framed_sha256: "c4801b75007386670b848bd858615dc07feedeb4527c052d8085312912a19054",
    },
    DatasetExpected {
        phase: "CREASTMT",
        name: "AWS.M2.CARDDEMO.STATEMNT.PS",
        width: 80,
        records: 1262,
        key: None,
        framed_sha256: "9c5af9e939c6e5779ef1c3c6680d284805d97f602ae7c4c45aa6e1007d168b46",
    },
    DatasetExpected {
        phase: "CREASTMT",
        name: "AWS.M2.CARDDEMO.STATEMNT.HTML",
        width: 100,
        records: 6632,
        key: None,
        framed_sha256: "f383ef6a1b2c5a3036a50bd094d873a0a26758e7d8b00c9cc7fc1c5f8e99898e",
    },
    DatasetExpected {
        phase: "TRANBKP",
        name: "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
        width: 350,
        records: 0,
        key: Some((0, 16)),
        framed_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    },
];

pub(super) const SPOOLS: &[SpoolExpected] = &[
    SpoolExpected {
        job: "POSTTRAN",
        file: "STEP15:SYSPRINT",
        records: 54,
        bytes: 3340,
        framed_sha256: "02813b97d0b2ddac74943775acbd2d8623bceafb1a4f80209d91abe6c5df9a44",
    },
    SpoolExpected {
        job: "INTCALC",
        file: "STEP15:SYSPRINT",
        records: 302,
        bytes: 10874,
        framed_sha256: "887941a0a7fe1fc7c3d297530e91e98355e38ddad5a2812d9bee54158ef8a602",
    },
    SpoolExpected {
        job: "TRANREPT",
        file: "STEP10R:SYSPRINT",
        records: 317,
        bytes: 109357,
        framed_sha256: "f11dfc5b48fcba8c091ccc0b159e3882219c19fce92bfc1c2e60c43f4db73fd8",
    },
];

pub(super) const FINAL_NAMES: &[&str] = &[
    "AWS.M2.CARDDEMO.ACCTDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CARDXREF.VSAM.KSDS",
    "AWS.M2.CARDDEMO.CUSTDATA.VSAM.KSDS",
    "AWS.M2.CARDDEMO.DALYREJS.G0001V00",
    "AWS.M2.CARDDEMO.DALYTRAN.PS",
    "AWS.M2.CARDDEMO.DISCGRP.VSAM.KSDS",
    "AWS.M2.CARDDEMO.STATEMNT.HTML",
    "AWS.M2.CARDDEMO.STATEMNT.PS",
    "AWS.M2.CARDDEMO.SYSTRAN.G0001V00",
    "AWS.M2.CARDDEMO.TCATBALF.BKUP.G0001V00",
    "AWS.M2.CARDDEMO.TCATBALF.REPT",
    "AWS.M2.CARDDEMO.TCATBALF.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRANCATG.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRANREPT.G0001V00",
    "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0001V00",
    "AWS.M2.CARDDEMO.TRANSACT.BKUP.G0002V00",
    "AWS.M2.CARDDEMO.TRANSACT.COMBINED.G0001V00",
    "AWS.M2.CARDDEMO.TRANSACT.DALY.G0001V00",
    "AWS.M2.CARDDEMO.TRANSACT.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRANTYPE.VSAM.KSDS",
    "AWS.M2.CARDDEMO.TRXFL.SEQ",
    "AWS.M2.CARDDEMO.TRXFL.VSAM.KSDS",
];
