import React, { useState } from "react";
import { Card } from "../../shared/ui/Card";
import { Input } from "../../shared/ui/Input";
import { Button } from "../../shared/ui/Button";

export default function LogsSearchPanel(){
    const [q,setQ]=useState("Bearer");
    const [res,setRes]=useState<any>();
    return (
        <div className="section grid">
            <div className="h1">Логи (публично)</div>
            <Card className="grid" style={{maxWidth:900}}>
                <div className="row">
                    <Input value={q} onChange={e=>setQ(e.target.value)} placeholder="поиск"/>
                    <Button onClick={async()=>{
                        const r = await fetch(`/api/logs/search?q=${encodeURIComponent(q)}`);
                        setRes(await r.json());
                    }}>Искать</Button>
                </div>
                {res && <pre className="card" style={{whiteSpace:"pre-wrap"}}>{JSON.stringify(res,null,2)}</pre>}
            </Card>
        </div>
    );
}
