import { Tab, TabList, TabPanel, Tabs as AriaTabs } from 'react-aria-components';
import './components.css';

export interface TabDef { id: string; label: string; content: React.ReactNode }

/** Tabs with roving focus (arrow keys), automatic activation, and an underline marker (not colour alone). */
export function Tabs({ label, tabs }: { label: string; tabs: TabDef[] }) {
  return (
    <AriaTabs className="tabs">
      <TabList aria-label={label} className="tablist">
        {tabs.map((t) => <Tab key={t.id} id={t.id} className="tab">{t.label}</Tab>)}
      </TabList>
      {tabs.map((t) => <TabPanel key={t.id} id={t.id} className="tabpanel">{t.content}</TabPanel>)}
    </AriaTabs>
  );
}
